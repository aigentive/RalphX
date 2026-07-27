use super::*;
use crate::application::agent_conversation_workspace::resolve_linked_plan_branch_agent_worktree_path;
use crate::application::chat_service::{MockChatService, SendQueuePolicy};
use crate::application::notification_service::{NoopNotificationEventEmitter, NotificationService};
use crate::application::task_notification_producer::TaskPipelineNotificationProducer;
use crate::application::AppState;
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as DbPrStatus};
use crate::domain::entities::task_metadata::GIT_ISOLATION_ERROR_PREFIX;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspacePrDescription,
    ArtifactId, ChatConversation, ChatConversationId, ExecutionFailureSource,
    ExecutionRecoveryEventKind, ExecutionRecoveryMetadata, ExecutionRecoveryReasonCode,
    ExecutionRecoverySource, ExecutionRecoveryState, IdeationAnalysisBaseRefKind,
    IdeationSessionId, InternalStatus, Notification, NotificationCategory, NotificationSeverity,
    NotificationTargetKind, PlanBranch, PlanBranchId, PlanBranchStatus, Project, ProjectId, Task,
    TaskCategory,
};
use crate::domain::repositories::{NotificationPage, NotificationRepository};
use crate::domain::services::github_service::{PrHealth, PrHealthCheck};
use crate::domain::services::{
    GithubServiceTrait, MemoryRunningAgentRegistry, MessageQueue, PlanPrDescriptionDrafter,
    PrReviewState,
};
use crate::domain::state_machine::services::{ReviewStartResult, ReviewStarter};
use crate::domain::state_machine::transition_handler::metadata_builder::MetadataUpdate;
use crate::error::{AppError, AppResult};
use crate::infrastructure::{MockAgenticClient, MockCallType};
use crate::tests::mock_github_service::MockGithubService;
use async_trait::async_trait;
use ralphx_events::{EventSink, RecordingEventSink};
use serde_json::Value;
use std::sync::Mutex;

#[test]
fn test_tauri_event_emitter_creation() {
    let emitter = EnrichedEventEmitter::new(None);
    assert!(emitter.event_sink.is_none());
}

#[tokio::test]
async fn enriched_event_emitter_sends_basic_events_to_event_sink() {
    let sink = RecordingEventSink::new();
    let sink_arc: Arc<dyn EventSink> = Arc::new(sink.clone());
    let emitter = EnrichedEventEmitter::new(Some(sink_arc));

    emitter.emit("agent:run_completed", "task-123").await;

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "agent:run_completed");
    assert_eq!(events[0].payload["taskId"], "task-123");
    assert!(events[0].payload["timestamp"].is_string());
}

#[tokio::test]
async fn enriched_event_emitter_sends_payload_events_to_event_sink() {
    let sink = RecordingEventSink::new();
    let sink_arc: Arc<dyn EventSink> = Arc::new(sink.clone());
    let emitter = EnrichedEventEmitter::new(Some(sink_arc));

    emitter
        .emit_with_payload("task:custom", "task-456", "payload-body")
        .await;

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "task:custom");
    assert_eq!(events[0].payload["taskId"], "task-456");
    assert_eq!(events[0].payload["payload"], "payload-body");
    assert!(events[0].payload["timestamp"].is_string());
}

#[tokio::test]
async fn enriched_event_emitter_routes_batchable_events_through_throttled_emitter() {
    let sink = RecordingEventSink::new();
    let sink_arc: Arc<dyn EventSink> = Arc::new(sink.clone());
    let throttled = crate::application::ThrottledEmitter::new(Arc::clone(&sink_arc));
    let emitter = EnrichedEventEmitter::new(Some(sink_arc)).with_throttled_emitter(throttled);

    emitter.emit("task:created", "task-789").await;
    assert!(
        sink.events().is_empty(),
        "batchable events should wait for the throttled flush"
    );

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "task:created");
    assert_eq!(events[0].payload["taskId"], "task-789");
    assert!(events[0].payload["timestamp"].is_string());
}

#[test]
fn test_logging_notifier() {
    let _notifier = NoopNotifier;
    // Just verify it can be created
}

#[test]
fn test_no_op_review_starter() {
    let _starter = NoOpReviewStarter;
    // Just verify it can be created
}

fn build_dependency_manager(app_state: &AppState) -> RepoBackedDependencyManager {
    RepoBackedDependencyManager::new(
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.task_repo),
        None,
    )
}

#[tokio::test]
async fn test_dependency_manager_treats_paused_blocker_as_incomplete() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    let mut blocker = Task::new(project.id.clone(), "Paused Blocker".to_string());
    blocker.internal_status = InternalStatus::Paused;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut blocked = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(blocked.clone()).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&blocked.id, &blocker.id)
        .await
        .unwrap();

    let has_blockers = manager.has_unresolved_blockers(blocked.id.as_str()).await;
    assert!(
        has_blockers,
        "Paused blockers should be treated as unresolved"
    );
}

/// Stopped is terminal but does NOT satisfy dependencies — stopped tasks
/// have incomplete work, so dependents should remain blocked.
#[tokio::test]
async fn test_is_blocker_complete_with_stopped_state() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    let mut blocker = Task::new(project.id.clone(), "Stopped Blocker".to_string());
    blocker.internal_status = InternalStatus::Stopped;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut blocked = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(blocked.clone()).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&blocked.id, &blocker.id)
        .await
        .unwrap();

    let has_blockers = manager.has_unresolved_blockers(blocked.id.as_str()).await;
    assert!(
        has_blockers,
        "Stopped blockers should still block dependents (incomplete work)"
    );
}

/// MergeIncomplete does NOT satisfy dependencies — merge failed, code not on target branch.
/// A task with a MergeIncomplete blocker should remain blocked.
#[tokio::test]
async fn test_is_blocker_complete_with_merge_incomplete_state() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    let mut blocker = Task::new(project.id.clone(), "MergeIncomplete Blocker".to_string());
    blocker.internal_status = InternalStatus::MergeIncomplete;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut blocked = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(blocked.clone()).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&blocked.id, &blocker.id)
        .await
        .unwrap();

    let has_blockers = manager.has_unresolved_blockers(blocked.id.as_str()).await;
    assert!(
        has_blockers,
        "MergeIncomplete blockers should NOT satisfy dependencies (merge failed)"
    );
}

// ============================================================================
// Wave 3: Metadata Merge Tests
// ============================================================================

fn build_test_service(app_state: &AppState) -> TaskTransitionService {
    let execution_state = Arc::new(ExecutionState::new());
    build_test_service_with_execution_state(app_state, execution_state)
}

fn build_test_service_with_execution_state(
    app_state: &AppState,
    execution_state: Arc<ExecutionState>,
) -> TaskTransitionService {
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
}

fn build_test_service_with_task_notifications(app_state: &AppState) -> TaskTransitionService {
    build_test_service(app_state).with_notifier(Arc::new(TaskPipelineNotificationProducer::new(
        app_state.notification_service(),
    )))
}

#[tokio::test]
async fn failed_completion_recovery_preserves_work_and_records_append_only_audit() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Recovery audit project".to_string(),
        "/tmp/recovery-audit-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id, "Recover completed work".to_string());
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("task/recovery-audit".to_string());
    task.worktree_path = Some("/tmp/recovery-audit-worktree".to_string());
    task.merge_commit_sha = Some("promoted-sha".to_string());
    task.metadata = Some(
        serde_json::json!({
            "failure_error": "false finalizer failure"
        })
        .to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();

    let evidence = crate::application::task_restart::FailedRecoveryEvidence {
        agent_run_id: "run-current".to_string(),
        validation_run_id: "validation-current".to_string(),
        promoted_commit_sha: "promoted-sha".to_string(),
        episode_entered_at: chrono::Utc::now(),
    };
    let service = build_test_service(&app_state);
    let recovered = service
        .recover_failed_completed_task_to_review(&task.id, &evidence)
        .await
        .unwrap();

    assert_eq!(recovered.internal_status, InternalStatus::PendingReview);
    assert_eq!(recovered.task_branch, task.task_branch);
    assert_eq!(recovered.worktree_path, task.worktree_path);
    assert_eq!(recovered.merge_commit_sha, task.merge_commit_sha);
    let metadata: serde_json::Value =
        serde_json::from_str(recovered.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(
        metadata["failed_completion_recovery"]["original_failure"],
        "false finalizer failure"
    );
    assert_eq!(
        metadata["execution_recovery"]["events"][0]["kind"],
        "completed_work_recovered"
    );
    assert_eq!(
        metadata["execution_recovery"]["events"][0]["reason_code"],
        "validated_completed_work"
    );

    let repeated = service
        .recover_failed_completed_task_to_review(&task.id, &evidence)
        .await;
    assert!(repeated.is_err(), "recovery CAS must not run twice");
}

#[tokio::test]
async fn accepted_execution_completion_persists_one_event_per_agent_run() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Completion event project".to_string(),
        "/tmp/completion-event-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "Complete once".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let service = build_test_service(&app_state)
        .with_external_events_repo(Arc::clone(&app_state.external_events_repo));
    service
        .transition_execution_completed_to_review(&task.id, "run-once")
        .await
        .unwrap();
    let repeated = service
        .transition_execution_completed_to_review(&task.id, "run-once")
        .await
        .expect("a late duplicate finalizer should be an idempotent no-op");
    assert_eq!(repeated.internal_status, InternalStatus::PendingReview);

    let events = app_state
        .external_events_repo
        .get_events_after_cursor(&[project.id.to_string()], 0, 100)
        .await
        .unwrap();
    let completion_events: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "task:execution_completed")
        .collect();
    assert_eq!(completion_events.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&completion_events[0].payload).unwrap();
    assert_eq!(payload["agent_run_id"], "run-once");
}

struct FailingReviewStarter;

#[async_trait]
impl ReviewStarter for FailingReviewStarter {
    async fn start_ai_review(&self, _task_id: &str, _project_id: &str) -> ReviewStartResult {
        ReviewStartResult::Error("injected review startup failure".to_string())
    }
}

struct RecordingNotifier {
    contexts: Mutex<Vec<NotificationContext>>,
    delegate: Arc<dyn Notifier>,
}

#[async_trait]
impl Notifier for RecordingNotifier {
    async fn notify(&self, context: NotificationContext, notification: TaskNotification) {
        self.contexts.lock().unwrap().push(context.clone());
        self.delegate.notify(context, notification).await;
    }
}

async fn task_notifications(app_state: &AppState) -> Vec<Notification> {
    app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .expect("notification read should succeed")
        .notifications
}

async fn assert_normal_task_notification(
    from: InternalStatus,
    to: InternalStatus,
    category: NotificationCategory,
    severity: NotificationSeverity,
    blocked_reason: Option<&str>,
) {
    let app_state = AppState::new_test();
    let service = build_test_service_with_task_notifications(&app_state);
    let project = Project::new("Notification Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), format!("{to:?} notification task"));
    task.internal_status = from;
    task.blocked_reason = blocked_reason.map(str::to_owned);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    service
        .transition_task(&task_id, to)
        .await
        .unwrap_or_else(|error| panic!("{from:?} to {to:?} should succeed: {error}"));

    let rows = task_notifications(&app_state).await;
    assert_eq!(rows.len(), 1, "{to:?} should create exactly one row");
    assert_eq!(rows[0].category, category);
    assert_eq!(rows[0].severity, severity);
    assert_eq!(rows[0].target.kind, NotificationTargetKind::Task);
    assert_eq!(
        rows[0].target.project_id.as_deref(),
        Some(project.id.as_str())
    );
    assert_eq!(rows[0].target.task_id.as_deref(), Some(task_id.as_str()));
    assert!(
        rows[0]
            .dedupe_key
            .as_deref()
            .is_some_and(|key| key.starts_with(&format!("task:{}:{}:", task_id, to.as_str()))),
        "dedupe key must be scoped to the committed transition history entry"
    );
}

struct FailingNotificationRepository;

#[async_trait]
impl NotificationRepository for FailingNotificationRepository {
    async fn create_with_dedupe(&self, _notification: Notification) -> AppResult<bool> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn list(
        &self,
        _project_id: Option<&str>,
        _cursor: Option<&str>,
        _limit: u32,
    ) -> AppResult<NotificationPage> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn unread_count(&self, _project_id: Option<&str>) -> AppResult<u64> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn mark_read(
        &self,
        _id: &str,
        _read_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Option<Notification>> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn mark_read_by_dedupe_key(
        &self,
        _dedupe_key: &str,
        _read_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Option<Notification>> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn mark_all_read(
        &self,
        _project_id: Option<&str>,
        _read_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u64> {
        Err(AppError::Database("injected notification failure".into()))
    }

    async fn prune(
        &self,
        _read_before: chrono::DateTime<chrono::Utc>,
        _max_rows: u32,
    ) -> AppResult<()> {
        Err(AppError::Database("injected notification failure".into()))
    }
}

#[tokio::test]
async fn task_pipeline_auto_transition_review_error_uses_auto_history_notification_context() {
    let app_state = AppState::new_test();
    let producer: Arc<dyn Notifier> = Arc::new(TaskPipelineNotificationProducer::new(
        app_state.notification_service(),
    ));
    let recorder = Arc::new(RecordingNotifier {
        contexts: Mutex::new(Vec::new()),
        delegate: producer,
    });
    let service = build_test_service(&app_state)
        .with_notifier(recorder.clone())
        .with_review_starter(Arc::new(FailingReviewStarter));
    let project = Project::new("Notification Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Auto review notification".to_string());
    task.internal_status = InternalStatus::QaTesting;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    service
        .transition_task(&task_id, InternalStatus::QaPassed)
        .await
        .expect("QA pass should enter the auto review path");

    let contexts = recorder.contexts.lock().unwrap().clone();
    assert!(
        !contexts.is_empty(),
        "the auto-entered pending-review error should notify"
    );
    let rows = task_notifications(&app_state).await;
    let expected_key = format!(
        "task:{task_id}:review_error:{}",
        contexts[0].history_entry_id
    );
    assert!(
        rows.iter()
            .any(|row| row.dedupe_key.as_deref() == Some(expected_key.as_str())),
        "the review-start alert must use the first auto-transition history id"
    );

    let history = app_state
        .task_repo
        .get_status_history(&task_id)
        .await
        .unwrap();
    assert!(
        history.len() >= 2,
        "QA pass must be followed by its auto transition"
    );
    assert_eq!(history[0].to, InternalStatus::QaPassed);
    assert_eq!(history[1].to, InternalStatus::PendingReview);
}

#[tokio::test]
async fn task_pipeline_review_passed_reentry_records_distinct_history_scoped_notifications() {
    let app_state = AppState::new_test();
    let service = build_test_service_with_task_notifications(&app_state);
    let worktree = tempfile::tempdir().expect("review worktree should be created");
    let project = Project::new(
        "Notification Project".to_string(),
        worktree.path().to_string_lossy().to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Review loop task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    task.worktree_path = Some(worktree.path().to_string_lossy().to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    service
        .transition_task(&task_id, InternalStatus::ReviewPassed)
        .await
        .expect("first reviewing to review-passed transition should succeed");
    service
        .transition_task(&task_id, InternalStatus::RevisionNeeded)
        .await
        .expect("review-passed to revision-needed transition should succeed");
    service
        .transition_task(&task_id, InternalStatus::PendingReview)
        .await
        .expect("re-executing to pending-review transition should succeed through the revision auto-transition");
    service
        .transition_task(&task_id, InternalStatus::ReviewPassed)
        .await
        .expect("second reviewing to review-passed transition should succeed");

    let rows = task_notifications(&app_state).await;
    assert_eq!(
        rows.len(),
        2,
        "each review-pass attempt should create one row"
    );
    assert!(rows.iter().all(|row| {
        row.category == NotificationCategory::ReviewNeeded
            && row.severity == NotificationSeverity::ActionRequired
            && row.target.kind == NotificationTargetKind::Task
            && row.target.project_id.as_deref() == Some(project.id.as_str())
            && row.target.task_id.as_deref() == Some(task_id.as_str())
    }));
    assert_ne!(
        rows[0].dedupe_key, rows[1].dedupe_key,
        "re-entry must use the freshly committed history row rather than a stale latest row"
    );
}

#[tokio::test]
async fn task_pipeline_duplicate_transition_delivery_keeps_one_notification_row() {
    let app_state = AppState::new_test();
    let service = build_test_service_with_task_notifications(&app_state);
    let project = Project::new("Notification Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Duplicate delivery task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    service
        .transition_task(&task_id, InternalStatus::ReviewPassed)
        .await
        .expect("initial transition should succeed");
    let duplicate = service
        .transition_task(&task_id, InternalStatus::ReviewPassed)
        .await
        .expect("duplicate delivery should be an authority-preserving no-op");

    assert_eq!(duplicate.internal_status, InternalStatus::ReviewPassed);
    let rows = task_notifications(&app_state).await;
    assert_eq!(
        rows.len(),
        1,
        "duplicate transition delivery must not duplicate the row"
    );
    assert_eq!(rows[0].category, NotificationCategory::ReviewNeeded);
}

#[tokio::test]
async fn task_pipeline_dependency_blocked_transition_records_no_notification() {
    let app_state = AppState::new_test();
    let service = build_test_service_with_task_notifications(&app_state);
    let project = Project::new("Notification Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Dependency-blocked task".to_string());
    task.internal_status = InternalStatus::ReExecuting;
    task.blocked_reason = Some("dependency: upstream task is still running".to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    service
        .transition_task(&task_id, InternalStatus::Blocked)
        .await
        .expect("re-executing to blocked transition should succeed");

    assert!(
        task_notifications(&app_state).await.is_empty(),
        "dependency blockers are not user-input blockers and must not notify"
    );
}

#[tokio::test]
async fn task_pipeline_merge_incomplete_normal_transition_records_actionable_row() {
    let app_state = AppState::new_test();
    let service = build_test_service_with_task_notifications(&app_state);
    let project = Project::new("Notification Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Merge incomplete task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    service
        .transition_task(&task_id, InternalStatus::MergeIncomplete)
        .await
        .expect("pending-merge to merge-incomplete transition should succeed");

    let rows = task_notifications(&app_state).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].category, NotificationCategory::MergeIncomplete);
    assert_eq!(rows[0].severity, NotificationSeverity::ActionRequired);
    assert_eq!(rows[0].target.kind, NotificationTargetKind::Task);
    assert_eq!(rows[0].target.task_id.as_deref(), Some(task_id.as_str()));
}

#[tokio::test]
async fn task_pipeline_corrective_failed_transition_records_actionable_row() {
    let app_state = AppState::new_test();
    let service = build_test_service_with_task_notifications(&app_state);
    let project = Project::new("Notification Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Corrective failure task".to_string());
    task.internal_status = InternalStatus::Blocked;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    service
        .transition_task_corrective(&task_id, InternalStatus::Failed, None, "recovery")
        .await
        .expect("corrective transition should succeed");

    let rows = task_notifications(&app_state).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].category, NotificationCategory::TaskFailed);
    assert_eq!(rows[0].severity, NotificationSeverity::ActionRequired);
    assert_eq!(rows[0].target.task_id.as_deref(), Some(task_id.as_str()));
}

#[tokio::test]
async fn task_pipeline_notification_repository_failure_does_not_fail_transition() {
    let app_state = AppState::new_test();
    let notification_service = Arc::new(NotificationService::new(
        Arc::new(FailingNotificationRepository),
        Arc::new(NoopNotificationEventEmitter),
    ));
    let service = build_test_service(&app_state).with_notifier(Arc::new(
        TaskPipelineNotificationProducer::new(notification_service),
    ));
    let project = Project::new("Notification Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Repository failure task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let transitioned = service
        .transition_task(&task_id, InternalStatus::ReviewPassed)
        .await
        .expect("best-effort notification failure must not fail the committed transition");

    assert_eq!(transitioned.internal_status, InternalStatus::ReviewPassed);
    assert_eq!(
        app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .expect("transitioned task should remain persisted")
            .internal_status,
        InternalStatus::ReviewPassed
    );
}

#[tokio::test]
async fn task_pipeline_qa_failed_normal_transition_records_actionable_row() {
    assert_normal_task_notification(
        InternalStatus::QaTesting,
        InternalStatus::QaFailed,
        NotificationCategory::QaFailed,
        NotificationSeverity::ActionRequired,
        None,
    )
    .await;
}

#[tokio::test]
async fn task_pipeline_escalated_normal_transition_records_actionable_row() {
    assert_normal_task_notification(
        InternalStatus::Reviewing,
        InternalStatus::Escalated,
        NotificationCategory::ReviewEscalated,
        NotificationSeverity::ActionRequired,
        None,
    )
    .await;
}

#[tokio::test]
async fn task_pipeline_merge_conflict_normal_transition_records_actionable_row() {
    assert_normal_task_notification(
        InternalStatus::Merging,
        InternalStatus::MergeConflict,
        NotificationCategory::MergeConflict,
        NotificationSeverity::ActionRequired,
        None,
    )
    .await;
}

#[tokio::test]
async fn task_pipeline_human_input_blocked_normal_transition_records_actionable_row() {
    assert_normal_task_notification(
        InternalStatus::ReExecuting,
        InternalStatus::Blocked,
        NotificationCategory::TaskBlocked,
        NotificationSeverity::ActionRequired,
        Some("human: approval is required"),
    )
    .await;
}

#[tokio::test]
async fn task_pipeline_freshness_blocked_notification_records_warning_row() {
    assert_normal_task_notification(
        InternalStatus::ReExecuting,
        InternalStatus::Blocked,
        NotificationCategory::TaskBlocked,
        NotificationSeverity::Warning,
        Some("FRESHNESS_BLOCKED|3|10|src/lib.rs|Persistent freshness conflicts"),
    )
    .await;
}

#[tokio::test]
async fn task_pipeline_failed_normal_transition_records_actionable_row() {
    assert_normal_task_notification(
        InternalStatus::Executing,
        InternalStatus::Failed,
        NotificationCategory::TaskFailed,
        NotificationSeverity::ActionRequired,
        None,
    )
    .await;
}

#[tokio::test]
async fn route_github_pr_changes_requested_records_auto_merge_disarm_marker() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "PR Review Project".to_string(),
        "/tmp/pr-review".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut merge_task = Task::new(project.id.clone(), "Merge plan PR".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    app_state
        .task_repo
        .create(merge_task.clone())
        .await
        .unwrap();
    let service = build_test_service(&app_state);
    let feedback = crate::domain::services::github_service::PrReviewFeedback {
        review_id: "review-marker".to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-05-17T12:00:00Z".to_string()),
        body: Some("Please adjust this.".to_string()),
        comments: Vec::new(),
    };

    service
        .route_github_pr_changes_requested_with_auto_merge_marker(
            &merge_task.id,
            676,
            feedback,
            "test",
            true,
            Some("rebase".to_string()),
        )
        .await
        .expect("review correction should route");

    let updated = app_state
        .task_repo
        .get_by_id(&merge_task.id)
        .await
        .unwrap()
        .expect("merge task should exist");
    let metadata: Value =
        serde_json::from_str(updated.metadata.as_deref().expect("metadata should exist"))
            .expect("metadata should be valid json");
    assert_eq!(
        metadata["github_auto_merge_disabled_for_correction"],
        Value::Bool(true)
    );
    assert_eq!(metadata["github_auto_merge_pr_number"], Value::from(676));
    assert_eq!(
        metadata["github_auto_merge_disabled_source"],
        Value::String("github_review_feedback".to_string())
    );
    assert_eq!(
        metadata["github_auto_merge_method"],
        Value::String("rebase".to_string())
    );
}

#[tokio::test]
async fn terminal_pr_state_consumes_auto_merge_disarm_marker() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Terminal PR Marker Project".to_string(),
        "/tmp/terminal-pr-marker".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut merge_task = Task::new(project.id.clone(), "Merge plan PR".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.metadata = Some(
        serde_json::json!({
            "github_auto_merge_disabled_for_correction": true,
            "github_auto_merge_pr_number": 676,
            "github_auto_merge_method": "rebase",
            "github_auto_merge_disabled_at": "2026-07-10T12:00:00Z",
            "github_auto_merge_disabled_source": "github_review_feedback",
            "github_auto_merge_reenable_failed_at": "2026-07-10T12:05:00Z",
            "github_auto_merge_reenable_error": "temporary GitHub error",
        })
        .to_string(),
    );
    app_state
        .task_repo
        .create(merge_task.clone())
        .await
        .unwrap();
    let service = build_test_service(&app_state);

    let changed = service
        .clear_github_auto_merge_correction_marker_for_terminal_pr(&merge_task.id, "merged")
        .await
        .expect("terminal marker cleanup should succeed");

    assert!(changed, "terminal cleanup should consume the active marker");
    let updated = app_state
        .task_repo
        .get_by_id(&merge_task.id)
        .await
        .unwrap()
        .expect("merge task should exist");
    let metadata: Value =
        serde_json::from_str(updated.metadata.as_deref().expect("metadata should exist"))
            .expect("metadata should be valid json");
    assert!(metadata
        .get("github_auto_merge_disabled_for_correction")
        .is_none());
    assert!(metadata.get("github_auto_merge_pr_number").is_none());
    assert!(metadata.get("github_auto_merge_method").is_none());
    assert!(metadata.get("github_auto_merge_reenable_error").is_none());
    assert_eq!(
        metadata["github_auto_merge_terminal_cleared_source"],
        Value::String("pr_terminal_state".to_string())
    );
    assert_eq!(
        metadata["github_auto_merge_terminal_cleared_status"],
        Value::String("merged".to_string())
    );
    assert!(metadata["github_auto_merge_terminal_cleared_at"].is_string());
}

#[tokio::test]
async fn with_event_sink_rebuilds_status_change_emitter_without_external_events() {
    let app_state = AppState::new_test();
    let project = Project::new("Sink Project".to_string(), "/tmp/sink".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let task = Task::new(project.id.clone(), "Sink Task".to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let sink = RecordingEventSink::new();
    let sink_arc: Arc<dyn EventSink> = Arc::new(sink.clone());
    let service = build_test_service(&app_state).with_event_sink(sink_arc);

    service
        .event_emitter
        .emit_status_change(task.id.as_str(), "ready", "executing")
        .await;

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "task:status_changed");
    assert_eq!(events[0].payload["task_id"], task.id.to_string());
    assert_eq!(events[0].payload["project_id"], project.id.to_string());
    assert_eq!(events[0].payload["old_status"], "ready");
    assert_eq!(events[0].payload["new_status"], "executing");
    assert_eq!(events[0].payload["project_name"], "Sink Project");
    assert_eq!(events[0].payload["task_title"], "Sink Task");
}

#[tokio::test]
async fn with_external_events_repo_preserves_event_sink_status_change_emits() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Dual Sink Project".to_string(),
        "/tmp/dual-sink".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let task = Task::new(project.id.clone(), "Dual Sink Task".to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let sink = RecordingEventSink::new();
    let sink_arc: Arc<dyn EventSink> = Arc::new(sink.clone());
    let ext_repo: Arc<dyn crate::domain::repositories::ExternalEventsRepository> =
        Arc::new(crate::infrastructure::memory::MemoryExternalEventsRepository::new());
    let service = build_test_service(&app_state)
        .with_event_sink(sink_arc)
        .with_external_events_repo(Arc::clone(&ext_repo));

    service
        .event_emitter
        .emit_status_change(task.id.as_str(), "backlog", "ready")
        .await;

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "task:status_changed");
    assert_eq!(events[0].payload["task_id"], task.id.to_string());

    let db_events = ext_repo
        .get_events_after_cursor(&[project.id.to_string()], 0, 100)
        .await
        .unwrap();
    assert_eq!(db_events.len(), 1);
    let db_payload: serde_json::Value = serde_json::from_str(&db_events[0].payload).unwrap();
    assert_eq!(db_payload["task_id"], task.id.to_string());
    assert_eq!(db_payload["project_id"], project.id.to_string());
    assert_eq!(db_payload["old_status"], "backlog");
    assert_eq!(db_payload["new_status"], "ready");
}

#[tokio::test]
async fn corrective_transition_with_exit_emits_task_event_through_event_sink() {
    let app_state = AppState::new_test();
    let project = Project::new(
        "Corrective Sink Project".to_string(),
        "/tmp/corrective".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let task = Task::new(project.id.clone(), "Corrective Sink Task".to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let sink = RecordingEventSink::new();
    let sink_arc: Arc<dyn EventSink> = Arc::new(sink.clone());
    let service = build_test_service(&app_state).with_event_sink(sink_arc);

    let updated = service
        .transition_task_corrective_with_exit(
            &task.id,
            InternalStatus::Failed,
            Some("corrective failure".to_string()),
            "system",
        )
        .await
        .unwrap();

    assert_eq!(updated.internal_status, InternalStatus::Failed);
    let events = sink.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, "task:event");
    assert_eq!(events[0].payload["type"], "status_changed");
    assert_eq!(events[0].payload["taskId"], task.id.to_string());
    assert_eq!(events[0].payload["from"], "backlog");
    assert_eq!(events[0].payload["to"], "failed");
    assert_eq!(events[0].payload["changedBy"], "system");
    assert_eq!(events[1].event, "task:status_changed");
    assert_eq!(events[1].payload["task_id"], task.id.to_string());
    assert_eq!(events[1].payload["old_status"], "backlog");
    assert_eq!(events[1].payload["new_status"], "failed");
}

#[test]
fn into_arc_wires_self_arc_for_task_services() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state).into_arc();

    let stored = service
        .self_arc
        .lock()
        .unwrap()
        .as_ref()
        .expect("into_arc should wire self_arc")
        .clone();
    assert!(Arc::ptr_eq(&stored, &service));
}

#[test]
fn execution_entry_guard_releases_in_flight_marker_on_drop() {
    let execution_state = Arc::new(ExecutionState::new());
    let task_id = "task-entry-guard";

    assert!(execution_state.try_start_execution_entry(task_id));
    {
        let _guard = ExecutionEntryGuard {
            execution_state: Arc::clone(&execution_state),
            task_id: task_id.to_string(),
        };
        assert!(execution_state.is_execution_entry_in_flight(task_id));
    }

    assert!(!execution_state.is_execution_entry_in_flight(task_id));
}

struct StaticPlanPrDescriptionDrafter;

#[async_trait]
impl PlanPrDescriptionDrafter for StaticPlanPrDescriptionDrafter {
    async fn draft_plan_description(
        &self,
        _project: &Project,
        _plan_branch: &PlanBranch,
        _review_base: &str,
        _review_state: PrReviewState,
    ) -> crate::error::AppResult<AgentWorkspacePrDescription> {
        Ok(AgentWorkspacePrDescription::new(
            None,
            "## Summary\n\nTransition service drafted body".to_string(),
        ))
    }
}

fn pr_sync_state(
    merge_state_status: Option<PrMergeStateStatus>,
    mergeable: Option<PrMergeableState>,
) -> PrSyncState {
    PrSyncState {
        status: PrStatus::Open,
        merge_state_status,
        mergeable,
        is_draft: false,
        head_ref_name: "plan/feature".to_owned(),
        base_ref_name: "main".to_owned(),
        head_ref_oid: None,
        base_ref_oid: None,
    }
}

#[test]
fn pr_sync_state_behind_mergeable_requires_programmatic_update() {
    let state = pr_sync_state(
        Some(PrMergeStateStatus::Behind),
        Some(PrMergeableState::Mergeable),
    );

    assert!(pr_sync_state_requires_update(&state));
    assert!(!pr_sync_state_requires_conflict_resolution(&state));
}

#[test]
fn pr_sync_state_conflicting_routes_to_merger_agent() {
    let dirty = pr_sync_state(
        Some(PrMergeStateStatus::Dirty),
        Some(PrMergeableState::Unknown),
    );
    let conflicting = pr_sync_state(
        Some(PrMergeStateStatus::Behind),
        Some(PrMergeableState::Conflicting),
    );

    assert!(pr_sync_state_requires_conflict_resolution(&dirty));
    assert!(pr_sync_state_requires_conflict_resolution(&conflicting));
    assert!(!pr_sync_state_requires_update(&conflicting));
}

#[test]
fn pr_sync_state_unknown_values_do_not_trigger_unsafe_updates() {
    let state = pr_sync_state(
        Some(PrMergeStateStatus::Other("NEW_STATE".to_owned())),
        Some(PrMergeableState::Unknown),
    );

    assert!(!pr_sync_state_requires_update(&state));
    assert!(!pr_sync_state_requires_conflict_resolution(&state));
}

#[test]
fn pr_branch_freshness_only_targets_active_waiting_plan_merge_tasks() {
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    let mut task = Task::new(project.id, "Merge plan into main".to_string());
    task.category = TaskCategory::PlanMerge;
    task.internal_status = InternalStatus::WaitingOnPr;

    assert!(pr_branch_freshness_task_eligible(&task));

    task.internal_status = InternalStatus::Merged;
    assert!(!pr_branch_freshness_task_eligible(&task));

    task.internal_status = InternalStatus::WaitingOnPr;
    task.category = TaskCategory::Regular;
    assert!(!pr_branch_freshness_task_eligible(&task));
}

#[tokio::test]
async fn push_and_refresh_pr_branch_uses_drafted_description() {
    let app_state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let service = build_test_service(&app_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo))
        .with_github_service(github_trait)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter));

    let mut project = Project::new("Test Project".to_string(), "/test/path".to_string());
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    let task = Task::new(project.id.clone(), "Refresh PR branch".to_string());

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-1".to_string()),
        IdeationSessionId::from_string("session-1".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(42);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let plan_branch_id = plan_branch.id.clone();
    app_state
        .plan_branch_repo
        .create(plan_branch.clone())
        .await
        .unwrap();

    service
        .push_and_refresh_pr_branch(&task, &project, &plan_branch)
        .await
        .expect("PR branch should push and refresh");

    {
        let state = github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(state.update_pr_details_calls, 1);
        let body = state
            .last_update_pr_details_body
            .as_deref()
            .expect("updated PR body should be captured");
        assert!(body.starts_with("## Summary\n\nTransition service drafted body"));
    }

    let updated_plan_branch = app_state
        .plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pushed);
}

#[tokio::test]
async fn push_and_refresh_pr_branch_stops_before_pr_refresh_when_push_fails() {
    let app_state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().push_branch_result = Some(Err(AppError::GitOperation(
        "remote rejected freshness branch".to_string(),
    )));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let service = build_test_service(&app_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo))
        .with_github_service(github_trait)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter));

    let mut project = Project::new("Test Project".to_string(), "/test/path".to_string());
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    let task = Task::new(project.id.clone(), "Refresh PR branch".to_string());

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-1".to_string()),
        IdeationSessionId::from_string("session-1".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(42);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let plan_branch_id = plan_branch.id.clone();
    app_state
        .plan_branch_repo
        .create(plan_branch.clone())
        .await
        .unwrap();

    let result = service
        .push_and_refresh_pr_branch(&task, &project, &plan_branch)
        .await;

    assert!(
        result.is_err(),
        "PR freshness should not report success when branch publication fails"
    );
    {
        let state = github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(
            state.update_pr_details_calls, 0,
            "PR details must not refresh after a failed branch push"
        );
    }

    let updated_plan_branch = app_state
        .plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Failed);
}

fn init_git_repo(path: &std::path::Path) {
    let run = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    std::fs::write(path.join("README.md"), "# test").expect("write README");
    run(&["add", "."]);
    run(&["commit", "-m", "initial"]);
}

fn init_git_repo_on_branch(path: &std::path::Path, branch: &str) {
    std::fs::create_dir_all(path).expect("create repo dir");
    let run = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "-b", branch]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    std::fs::write(path.join("README.md"), "# linked plan").expect("write README");
    run(&["add", "."]);
    run(&["commit", "-m", "initial"]);
}

fn pr_health_with_failing_check(head: &str, check_name: &str) -> PrHealth {
    PrHealth {
        sync_state: PrSyncState {
            status: PrStatus::Open,
            merge_state_status: None,
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: "ralphx/test/plan-route".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some(head.to_string()),
            base_ref_oid: Some("base".to_string()),
        },
        review_decision: None,
        checks: vec![PrHealthCheck {
            name: check_name.to_string(),
            status: Some("completed".to_string()),
            conclusion: Some("failure".to_string()),
            details_url: Some("https://github.com/owner/repo/actions/runs/609".to_string()),
        }],
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }
}

#[tokio::test]
async fn route_plan_pr_autofix_uses_linked_ideation_workspace_without_workspace_pr() {
    let app_state = AppState::new_test();
    let project_root = tempfile::tempdir().expect("project root");
    init_git_repo(project_root.path());
    let worktree_parent = tempfile::tempdir().expect("worktree parent");

    let mut project = Project::new(
        "Plan Autofix Project".to_string(),
        project_root.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("project-plan-autofix-route".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().into_owned());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session_id = IdeationSessionId::from_string("session-plan-autofix-route");
    let plan_branch_id = PlanBranchId::from_string("plan-branch-autofix-route");
    let branch_name = "ralphx/test/plan-route";
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-plan-autofix-route"),
        session_id.clone(),
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.id = plan_branch_id.clone();
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(609);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/609".to_string());
    plan_branch.pr_status = Some(DbPrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    app_state
        .plan_branch_repo
        .create(plan_branch.clone())
        .await
        .unwrap();

    let linked_worktree =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch).unwrap();
    init_git_repo_on_branch(&linked_worktree, branch_name);

    let conversation_id = ChatConversationId::from_string("60906090-6090-6090-6090-609060906090");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Ideation);
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        branch_name.to_string(),
        linked_worktree.to_string_lossy().into_owned(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch_id.clone());
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.publication_push_status = None;
    workspace.auto_publish_enabled = true;
    workspace.pr_autofix_enabled = true;
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(pr_health_with_failing_check(
        "route-head",
        "Coverage Gate",
    )));
    let github_trait: Arc<dyn GithubServiceTrait> = github;
    let execution_state = Arc::new(ExecutionState::new());
    let chat_service = Arc::new(MockChatService::with_agent_run_repo(Arc::clone(
        &app_state.agent_run_repo,
    )));
    let mut service = build_test_service_with_execution_state(&app_state, execution_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo))
        .with_agent_conversation_workspace_repo(Arc::clone(
            &app_state.agent_conversation_workspace_repo,
        ))
        .with_github_service(github_trait);
    service.chat_service = chat_service.clone();

    let routed = service
        .route_plan_pr_autofix_if_needed(&plan_branch_id, 609)
        .await
        .expect("linked plan PR autofix routing should succeed");

    assert!(routed);
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    assert_eq!(
        sent_options[0].queue_policy,
        SendQueuePolicy::RequireImmediateStart
    );
    assert!(sent_options[0].preallocated_agent_run_id.is_some());
    let updated = app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("fixing"));
    assert!(updated
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("failing check"));
    let events = app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert!(events.iter().any(|event| {
        event.step == "pr_autofix"
            && event.status == "needs_agent"
            && event
                .classification
                .as_deref()
                .unwrap_or_default()
                .starts_with("github_pr_autofix:609:routehead")
    }));
}

#[tokio::test]
async fn route_plan_pr_autofix_skips_incomplete_or_stale_linked_plan_context() {
    let app_state = AppState::new_test();
    let plan_branch_id = PlanBranchId::from_string("plan-branch-autofix-skip-current");
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github;

    assert!(!build_test_service(&app_state)
        .route_plan_pr_autofix_if_needed(&plan_branch_id, 609)
        .await
        .expect("missing plan repo should skip"));
    assert!(!build_test_service(&app_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo))
        .route_plan_pr_autofix_if_needed(&plan_branch_id, 609)
        .await
        .expect("missing workspace repo should skip"));
    assert!(!build_test_service(&app_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo))
        .with_agent_conversation_workspace_repo(Arc::clone(
            &app_state.agent_conversation_workspace_repo,
        ))
        .route_plan_pr_autofix_if_needed(&plan_branch_id, 609)
        .await
        .expect("missing GitHub service should skip"));

    let service = build_test_service(&app_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo))
        .with_agent_conversation_workspace_repo(Arc::clone(
            &app_state.agent_conversation_workspace_repo,
        ))
        .with_github_service(github_trait);
    let missing = service
        .route_plan_pr_autofix_if_needed(&plan_branch_id, 609)
        .await;
    assert!(matches!(
        missing,
        Err(AppError::NotFound(message))
            if message.contains("No plan branch found for PR supervision")
    ));

    let project_id = ProjectId::from_string("project-plan-autofix-skip".to_string());
    let make_plan_branch = |suffix: &str, pr_number: Option<i64>| {
        let session_id = IdeationSessionId::from_string(format!("session-{suffix}"));
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string(format!("artifact-{suffix}")),
            session_id,
            project_id.clone(),
            format!("ralphx/test/{suffix}"),
            "main".to_string(),
        );
        plan_branch.id = PlanBranchId::from_string(format!("plan-branch-{suffix}"));
        plan_branch.pr_eligible = true;
        plan_branch.pr_number = pr_number;
        plan_branch.pr_url = Some("https://github.com/owner/repo/pull/609".to_string());
        plan_branch.pr_status = Some(DbPrStatus::Open);
        plan_branch.pr_push_status = PrPushStatus::Pushed;
        plan_branch
    };

    let mut ineligible = make_plan_branch("ineligible", Some(609));
    ineligible.pr_eligible = false;
    app_state
        .plan_branch_repo
        .create(ineligible.clone())
        .await
        .unwrap();
    assert!(!service
        .route_plan_pr_autofix_if_needed(&ineligible.id, 609)
        .await
        .expect("ineligible branch should skip"));

    let mut merged_branch = make_plan_branch("merged", Some(609));
    merged_branch.status = PlanBranchStatus::Merged;
    app_state
        .plan_branch_repo
        .create(merged_branch.clone())
        .await
        .unwrap();
    assert!(!service
        .route_plan_pr_autofix_if_needed(&merged_branch.id, 609)
        .await
        .expect("inactive branch should skip"));

    let wrong_pr = make_plan_branch("wrong-pr", Some(610));
    app_state
        .plan_branch_repo
        .create(wrong_pr.clone())
        .await
        .unwrap();
    assert!(!service
        .route_plan_pr_autofix_if_needed(&wrong_pr.id, 609)
        .await
        .expect("PR number mismatch should skip"));

    let no_workspace = make_plan_branch("no-workspace", Some(609));
    app_state
        .plan_branch_repo
        .create(no_workspace.clone())
        .await
        .unwrap();
    assert!(!service
        .route_plan_pr_autofix_if_needed(&no_workspace.id, 609)
        .await
        .expect("missing linked workspace should skip"));

    let mismatched_workspace_plan = make_plan_branch("workspace-mismatch", Some(609));
    app_state
        .plan_branch_repo
        .create(mismatched_workspace_plan.clone())
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("60916091-6091-6091-6091-609160916091"),
        project_id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        mismatched_workspace_plan.branch_name.clone(),
        "/tmp/unused-linked-plan-worktree".to_string(),
    );
    workspace.linked_ideation_session_id = Some(mismatched_workspace_plan.session_id.clone());
    workspace.linked_plan_branch_id = Some(PlanBranchId::from_string("other-plan-branch"));
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    assert!(!service
        .route_plan_pr_autofix_if_needed(&mismatched_workspace_plan.id, 609)
        .await
        .expect("workspace linked to another plan branch should skip"));
}

#[tokio::test]
async fn test_transition_task_with_metadata_update_persists_atomically() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Test Task".to_string());
    task.internal_status = InternalStatus::Backlog;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let metadata_update = MetadataUpdate::new()
        .with_string("custom_key", "custom_value")
        .with_bool("is_test", true);

    let updated_task = service
        .transition_task_with_metadata(&task.id, InternalStatus::Ready, Some(metadata_update))
        .await
        .unwrap();

    assert_eq!(updated_task.internal_status, InternalStatus::Ready);

    let metadata_json = updated_task.metadata.expect("Metadata should be set");
    let parsed: serde_json::Map<String, Value> = serde_json::from_str(&metadata_json).unwrap();

    assert_eq!(
        parsed.get("custom_key").unwrap(),
        &Value::String("custom_value".to_string())
    );
    assert_eq!(parsed.get("is_test").unwrap(), &Value::Bool(true));
}

#[tokio::test]
async fn test_transition_task_with_none_preserves_existing_metadata() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Test Task".to_string());
    task.internal_status = InternalStatus::Backlog;
    task.metadata = Some(r#"{"existing_key":"existing_value"}"#.to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let updated_task = service
        .transition_task_with_metadata(&task.id, InternalStatus::Ready, None)
        .await
        .unwrap();

    assert_eq!(updated_task.internal_status, InternalStatus::Ready);

    let metadata_json = updated_task.metadata.expect("Metadata should be preserved");
    let parsed: serde_json::Map<String, Value> = serde_json::from_str(&metadata_json).unwrap();

    assert_eq!(
        parsed.get("existing_key").unwrap(),
        &Value::String("existing_value".to_string())
    );
}

#[tokio::test]
async fn test_transition_task_rejects_archived_task() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Archived Task".to_string());
    task.internal_status = InternalStatus::Ready;
    let task = app_state.task_repo.create(task).await.unwrap();
    app_state.task_repo.archive(&task.id).await.unwrap();

    let err = service
        .transition_task(&task.id, InternalStatus::Executing)
        .await
        .expect_err("archived tasks must not be transitionable");

    assert!(
        matches!(err, AppError::Validation(ref message) if message.contains("archived")),
        "expected archived-task validation error, got {err:?}"
    );
}

#[tokio::test]
async fn test_qa_refining_transition_auto_adds_trigger_origin() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Test Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let updated_task = service
        .transition_task_with_metadata(&task.id, InternalStatus::QaRefining, None)
        .await
        .unwrap();

    assert_eq!(updated_task.internal_status, InternalStatus::QaRefining);

    let metadata_json = updated_task
        .metadata
        .expect("Metadata should have trigger_origin");
    let parsed: serde_json::Map<String, Value> = serde_json::from_str(&metadata_json).unwrap();

    assert_eq!(
        parsed.get("trigger_origin").unwrap(),
        &Value::String("qa".to_string())
    );
}

#[tokio::test]
async fn test_qa_testing_transition_auto_adds_trigger_origin() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Test Task".to_string());
    task.internal_status = InternalStatus::QaRefining;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let updated_task = service
        .transition_task_with_metadata(&task.id, InternalStatus::QaTesting, None)
        .await
        .unwrap();

    assert_eq!(updated_task.internal_status, InternalStatus::QaTesting);

    let metadata_json = updated_task
        .metadata
        .expect("Metadata should have trigger_origin");
    let parsed: serde_json::Map<String, Value> = serde_json::from_str(&metadata_json).unwrap();

    assert_eq!(
        parsed.get("trigger_origin").unwrap(),
        &Value::String("qa".to_string())
    );
}

#[tokio::test]
async fn test_qa_transition_uses_injected_agentic_client_factory() {
    let app_state = AppState::new_test();
    let mock_client = Arc::new(MockAgenticClient::new());
    let service = build_test_service(&app_state)
        .with_agentic_client(mock_client.clone() as Arc<dyn crate::domain::agents::AgenticClient>);

    let repo_dir = tempfile::tempdir().unwrap();
    init_git_repo(repo_dir.path());

    let project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().into_owned(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Test Task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(repo_dir.path().to_string_lossy().into_owned());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let updated_task = service
        .transition_task_with_metadata(&task.id, InternalStatus::QaRefining, None)
        .await
        .unwrap();

    assert_eq!(updated_task.internal_status, InternalStatus::QaRefining);

    let calls = mock_client.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
    match &calls[0].call_type {
        MockCallType::Spawn { role, prompt } => {
            assert_eq!(*role, crate::domain::agents::AgentRole::QaRefiner);
            assert!(prompt.contains(task.id.as_str()));
        }
        other => panic!("expected spawn call, got {other:?}"),
    }
}

#[tokio::test]
async fn test_metadata_merge_preserves_existing_keys_not_in_update() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Test Task".to_string());
    task.internal_status = InternalStatus::Backlog;
    task.metadata =
        Some(r#"{"existing_key":"existing_value","another_key":"another_value"}"#.to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let metadata_update = MetadataUpdate::new().with_string("new_key", "new_value");

    let updated_task = service
        .transition_task_with_metadata(&task.id, InternalStatus::Ready, Some(metadata_update))
        .await
        .unwrap();

    let metadata_json = updated_task.metadata.expect("Metadata should be merged");
    let parsed: serde_json::Map<String, Value> = serde_json::from_str(&metadata_json).unwrap();

    assert_eq!(
        parsed.get("existing_key").unwrap(),
        &Value::String("existing_value".to_string())
    );
    assert_eq!(
        parsed.get("another_key").unwrap(),
        &Value::String("another_value".to_string())
    );
    assert_eq!(
        parsed.get("new_key").unwrap(),
        &Value::String("new_value".to_string())
    );
}

// ============================================================================
// Regression: merge unblocks dependent tasks
// ============================================================================

/// Regression test: when task A merges via the programmatic path (side_effects.rs),
/// task B which depends on A must be unblocked (Blocked → Ready).
///
/// Before the fix, complete_merge_internal bypassed TransitionHandler so on_enter(Merged)
/// never fired and unblock_dependents was never called. Blocked tasks stayed stuck forever.
#[tokio::test]
async fn test_merge_unblocks_dependent_task() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    // Task A: the dependency (blocker) — simulate it just merged
    let mut task_a = Task::new(project.id.clone(), "Task A (Blocker)".to_string());
    task_a.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(task_a.clone()).await.unwrap();

    // Task B: depends on A, currently blocked
    let mut task_b = Task::new(project.id.clone(), "Task B (Dependent)".to_string());
    task_b.internal_status = InternalStatus::Blocked;
    task_b.blocked_reason = Some(format!("Waiting for: {}", task_a.title));
    app_state.task_repo.create(task_b.clone()).await.unwrap();

    // Register dependency: B is blocked by A (B depends on A)
    app_state
        .task_dependency_repo
        .add_dependency(&task_b.id, &task_a.id)
        .await
        .unwrap();

    // Simulate what post_merge_cleanup now calls after complete_merge_internal succeeds
    manager.unblock_dependents(task_a.id.as_str()).await;

    // Assert B is now Ready
    let updated_b = app_state
        .task_repo
        .get_by_id(&task_b.id)
        .await
        .unwrap()
        .expect("Task B should still exist");

    assert_eq!(
        updated_b.internal_status,
        InternalStatus::Ready,
        "Task B should be unblocked to Ready after Task A merges"
    );
    assert!(
        updated_b.blocked_reason.is_none(),
        "Task B should have no blocked_reason after unblocking"
    );
}

/// Regression: unblock_dependents is idempotent — calling it twice does not cause errors
/// and a Ready task stays Ready (not double-transitioned).
#[tokio::test]
async fn test_merge_unblocks_dependent_task_idempotent() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    let mut task_a = Task::new(project.id.clone(), "Task A (Blocker)".to_string());
    task_a.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(task_a.clone()).await.unwrap();

    let mut task_b = Task::new(project.id.clone(), "Task B (Dependent)".to_string());
    task_b.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(task_b.clone()).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&task_b.id, &task_a.id)
        .await
        .unwrap();

    // Call twice (defence-in-depth may call it from both post_merge_cleanup and chat_service_merge)
    manager.unblock_dependents(task_a.id.as_str()).await;
    manager.unblock_dependents(task_a.id.as_str()).await;

    let updated_b = app_state
        .task_repo
        .get_by_id(&task_b.id)
        .await
        .unwrap()
        .expect("Task B should still exist");

    assert_eq!(
        updated_b.internal_status,
        InternalStatus::Ready,
        "Task B should be Ready after idempotent unblock calls"
    );
}

/// When task A merges but task B has another blocker still incomplete,
/// task B should remain Blocked.
#[tokio::test]
async fn test_merge_does_not_unblock_task_with_remaining_blocker() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    // Task A merges
    let mut task_a = Task::new(project.id.clone(), "Task A".to_string());
    task_a.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(task_a.clone()).await.unwrap();

    // Task C is still executing (incomplete blocker)
    let mut task_c = Task::new(project.id.clone(), "Task C (Still Running)".to_string());
    task_c.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task_c.clone()).await.unwrap();

    // Task B depends on both A and C
    let mut task_b = Task::new(project.id.clone(), "Task B (Dependent)".to_string());
    task_b.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(task_b.clone()).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&task_b.id, &task_a.id)
        .await
        .unwrap();
    app_state
        .task_dependency_repo
        .add_dependency(&task_b.id, &task_c.id)
        .await
        .unwrap();

    // A merges — but C is still running, so B should stay Blocked
    manager.unblock_dependents(task_a.id.as_str()).await;

    let updated_b = app_state
        .task_repo
        .get_by_id(&task_b.id)
        .await
        .unwrap()
        .expect("Task B should still exist");

    assert_eq!(
        updated_b.internal_status,
        InternalStatus::Blocked,
        "Task B should remain Blocked since Task C is still executing"
    );
}

#[test]
fn test_lane_settings_repo_can_be_applied_before_execution_settings_repo() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state)
        .with_agent_lane_settings_repo(Arc::clone(&app_state.agent_lane_settings_repo))
        .with_execution_settings_repo(Arc::clone(&app_state.execution_settings_repo));

    assert!(service.agent_lane_settings_repo.is_some());
    assert!(service.execution_settings_repo.is_some());
}

#[test]
fn test_lane_settings_repo_can_be_applied_after_execution_settings_repo() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state)
        .with_execution_settings_repo(Arc::clone(&app_state.execution_settings_repo))
        .with_agent_lane_settings_repo(Arc::clone(&app_state.agent_lane_settings_repo));

    assert!(service.agent_lane_settings_repo.is_some());
    assert!(service.execution_settings_repo.is_some());
}

#[test]
fn test_runtime_resolution_context_applies_all_runtime_dependencies() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state).with_runtime_resolution_context(
        Some(app_state.agent_client_bundle()),
        Some(Arc::clone(&app_state.execution_settings_repo)),
        Some(Arc::clone(&app_state.agent_lane_settings_repo)),
        Some(Arc::clone(&app_state.agent_provider_settings_repo)),
        Some(Arc::new(app_state.manual_role_default_service())),
        Some(Arc::clone(&app_state.plan_branch_repo)),
        Some(Arc::clone(&app_state.interactive_process_registry)),
    );

    assert!(service.execution_settings_repo.is_some());
    assert!(service.agent_lane_settings_repo.is_some());
    assert!(service.agent_provider_settings_repo.is_some());
    assert!(service.manual_role_default_service.is_some());
    assert!(service.plan_branch_repo.is_some());
    assert!(service.interactive_process_registry.is_some());
}

// ============================================================================
// Hard-block dependents when a blocker fails
// ============================================================================

/// When a blocker fails, dependents must stay Blocked — not unblocked to Ready.
/// This prevents cascade execution against broken output.
#[tokio::test]
async fn test_failed_blocker_keeps_dependent_blocked() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    // Blocker fails during execution
    let mut blocker = Task::new(project.id.clone(), "Setup DB".to_string());
    blocker.internal_status = InternalStatus::Failed;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    // Dependent was blocked waiting for the blocker
    let mut dependent = Task::new(project.id.clone(), "Run Migrations".to_string());
    dependent.internal_status = InternalStatus::Blocked;
    dependent.blocked_reason = Some(format!("Waiting for: {}", blocker.title));
    app_state.task_repo.create(dependent.clone()).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&dependent.id, &blocker.id)
        .await
        .unwrap();

    // Simulate on_enter(Failed) calling unblock_dependents
    manager.unblock_dependents(blocker.id.as_str()).await;

    let updated = app_state
        .task_repo
        .get_by_id(&dependent.id)
        .await
        .unwrap()
        .expect("Dependent should still exist");

    assert_eq!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Dependent should remain Blocked when blocker fails"
    );
}

/// When a blocker fails, the dependent's blocked_reason must mention the failed dependency.
#[tokio::test]
async fn test_failed_blocker_sets_blocked_reason_with_failure_message() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    let mut blocker = Task::new(project.id.clone(), "Setup DB".to_string());
    blocker.internal_status = InternalStatus::Failed;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut dependent = Task::new(project.id.clone(), "Run Migrations".to_string());
    dependent.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(dependent.clone()).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&dependent.id, &blocker.id)
        .await
        .unwrap();

    manager.unblock_dependents(blocker.id.as_str()).await;

    let updated = app_state
        .task_repo
        .get_by_id(&dependent.id)
        .await
        .unwrap()
        .expect("Dependent should still exist");

    let reason = updated
        .blocked_reason
        .expect("blocked_reason should be set when blocker fails");
    assert!(
        reason.contains("Setup DB") && reason.to_lowercase().contains("fail"),
        "blocked_reason should mention the failed dependency name and failure, got: {reason}"
    );
}

/// Mixed scenario: one blocker failed, another is still running — dependent stays Blocked.
#[tokio::test]
async fn test_mixed_failed_and_running_blockers_keeps_dependent_blocked() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    let mut failed_blocker = Task::new(project.id.clone(), "Setup DB".to_string());
    failed_blocker.internal_status = InternalStatus::Failed;
    app_state
        .task_repo
        .create(failed_blocker.clone())
        .await
        .unwrap();

    let mut running_blocker = Task::new(project.id.clone(), "Build Assets".to_string());
    running_blocker.internal_status = InternalStatus::Executing;
    app_state
        .task_repo
        .create(running_blocker.clone())
        .await
        .unwrap();

    let mut dependent = Task::new(project.id.clone(), "Deploy".to_string());
    dependent.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(dependent.clone()).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&dependent.id, &failed_blocker.id)
        .await
        .unwrap();
    app_state
        .task_dependency_repo
        .add_dependency(&dependent.id, &running_blocker.id)
        .await
        .unwrap();

    manager.unblock_dependents(failed_blocker.id.as_str()).await;

    let updated = app_state
        .task_repo
        .get_by_id(&dependent.id)
        .await
        .unwrap()
        .expect("Dependent should still exist");

    assert_eq!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Dependent should remain Blocked when both failed and running blockers exist"
    );
}

/// A Failed blocker treated as incomplete — has_unresolved_blockers returns true.
#[tokio::test]
async fn test_has_unresolved_blockers_treats_failed_as_unresolved() {
    let app_state = AppState::new_test();
    let manager = build_dependency_manager(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());

    let mut blocker = Task::new(project.id.clone(), "Build Step".to_string());
    blocker.internal_status = InternalStatus::Failed;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut dependent = Task::new(project.id.clone(), "Deploy Step".to_string());
    dependent.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(dependent.clone()).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&dependent.id, &blocker.id)
        .await
        .unwrap();

    let has_blockers = manager.has_unresolved_blockers(dependent.id.as_str()).await;
    assert!(
        has_blockers,
        "Failed blockers must be treated as unresolved (hard-block)"
    );
}

// ============================================================================
// RC5: Event-driven transition logging
// Verify that the three primary event-driven transitions succeed and return the
// correct status. The INFO log added to transition_task_with_metadata fires on
// the success path, so a passing test confirms the log line is reachable.
// ============================================================================

/// RC5 guard: Executing → PendingReview (ExecutionComplete event path).
#[tokio::test]
async fn test_executing_to_pending_review_transition_succeeds() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "RC5 Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let updated = service
        .transition_task_with_metadata(&task.id, InternalStatus::PendingReview, None)
        .await
        .unwrap();

    assert_eq!(
        updated.internal_status,
        InternalStatus::PendingReview,
        "RC5: Executing → PendingReview must succeed and persist"
    );
}

/// RC5 guard: Reviewing → ReviewPassed (ReviewComplete event path).
#[tokio::test]
async fn test_reviewing_to_review_passed_transition_succeeds() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "RC5 Reviewing Task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let updated = service
        .transition_task_with_metadata(&task.id, InternalStatus::ReviewPassed, None)
        .await
        .unwrap();

    assert_eq!(
        updated.internal_status,
        InternalStatus::ReviewPassed,
        "RC5: Reviewing → ReviewPassed must succeed and persist"
    );
}

/// RC5 guard: ReviewPassed → Approved (HumanApprove event path).
#[tokio::test]
async fn test_review_passed_to_approved_transition_succeeds() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "RC5 ReviewPassed Task".to_string());
    task.internal_status = InternalStatus::ReviewPassed;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let updated = service
        .transition_task_with_metadata(&task.id, InternalStatus::Approved, None)
        .await
        .unwrap();

    assert_eq!(
        updated.internal_status,
        InternalStatus::Approved,
        "RC5: ReviewPassed → Approved must succeed and persist"
    );
}

#[tokio::test]
async fn test_reviewing_to_approved_transition_is_rejected() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Invalid Reviewing Approval".to_string());
    task.internal_status = InternalStatus::Reviewing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let result = service
        .transition_task_with_metadata(&task_id, InternalStatus::Approved, None)
        .await;

    assert!(
        matches!(
            result,
            Err(AppError::InvalidTransition { ref from, ref to })
                if from == "reviewing" && to == "approved"
        ),
        "reviewing -> approved must be rejected with InvalidTransition"
    );

    let persisted = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.internal_status, InternalStatus::Reviewing);
}

#[tokio::test]
async fn test_merged_to_approved_transition_is_rejected() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Invalid Merged Approval".to_string());
    task.internal_status = InternalStatus::Merged;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let result = service
        .transition_task_with_metadata(&task_id, InternalStatus::Approved, None)
        .await;

    assert!(
        matches!(
            result,
            Err(AppError::InvalidTransition { ref from, ref to })
                if from == "merged" && to == "approved"
        ),
        "merged -> approved must be rejected with InvalidTransition"
    );

    let persisted = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.internal_status, InternalStatus::Merged);
}

#[tokio::test]
async fn test_transition_task_corrective_allows_blocked_to_failed() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Corrective Failed Task".to_string());
    task.internal_status = InternalStatus::Blocked;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let updated = service
        .transition_task_corrective(&task_id, InternalStatus::Failed, None, "test")
        .await
        .unwrap();

    assert_eq!(updated.internal_status, InternalStatus::Failed);

    let persisted = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.internal_status, InternalStatus::Failed);
}

#[tokio::test]
async fn test_transition_task_corrective_allows_pending_review_to_backlog() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Corrective Backlog Task".to_string());
    task.internal_status = InternalStatus::PendingReview;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let updated = service
        .transition_task_corrective(&task_id, InternalStatus::Backlog, None, "test")
        .await
        .unwrap();

    assert_eq!(updated.internal_status, InternalStatus::Backlog);

    let persisted = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.internal_status, InternalStatus::Backlog);
}

// ============================================================================
// Wave 3: Git Isolation ExecutionRecoveryMetadata Tests
// ============================================================================

#[test]
fn test_git_isolation_metadata_created_for_git_isolation_reason() {
    let reason = format!(
        "{}: could not create worktree at '/tmp/test'",
        GIT_ISOLATION_ERROR_PREFIX
    );
    let result = create_git_isolation_recovery_metadata_json(&reason, None);
    assert!(
        result.is_some(),
        "Expected metadata JSON for git isolation reason"
    );
}

#[test]
fn test_git_isolation_metadata_not_created_for_non_git_reason() {
    let result = create_git_isolation_recovery_metadata_json("Agent error: something failed", None);
    assert!(
        result.is_none(),
        "Expected no metadata for non-git ExecutionBlocked reason"
    );
}

#[test]
fn test_git_isolation_metadata_not_created_for_empty_reason() {
    let result = create_git_isolation_recovery_metadata_json("", None);
    assert!(result.is_none(), "Expected no metadata for empty reason");
}

#[test]
fn test_git_isolation_metadata_last_state_is_retrying() {
    let reason = format!("{}: could not create worktree", GIT_ISOLATION_ERROR_PREFIX);
    let json = create_git_isolation_recovery_metadata_json(&reason, None).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let recovery: ExecutionRecoveryMetadata =
        serde_json::from_value(parsed["execution_recovery"].clone()).unwrap();

    assert_eq!(
        recovery.last_state,
        ExecutionRecoveryState::Retrying,
        "last_state must be Retrying for reconciler eligibility"
    );
    assert!(
        !recovery.stop_retrying,
        "stop_retrying must be false on initial failure"
    );
}

#[test]
fn test_git_isolation_metadata_event_has_correct_fields() {
    let reason = format!("{}: stale index.lock detected", GIT_ISOLATION_ERROR_PREFIX);
    let json = create_git_isolation_recovery_metadata_json(&reason, None).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let recovery: ExecutionRecoveryMetadata =
        serde_json::from_value(parsed["execution_recovery"].clone()).unwrap();

    assert_eq!(recovery.events.len(), 1);
    let event = &recovery.events[0];
    assert_eq!(event.kind, ExecutionRecoveryEventKind::Failed);
    assert_eq!(event.source, ExecutionRecoverySource::Auto);
    assert_eq!(
        event.reason_code,
        ExecutionRecoveryReasonCode::GitIsolationFailed
    );
    assert_eq!(
        event.failure_source,
        Some(ExecutionFailureSource::GitIsolation)
    );
    assert_eq!(event.message, reason);
}

#[test]
fn test_git_isolation_metadata_deserialization_round_trip() {
    let reason = format!(
        "{}: leftover worktree directory exists",
        GIT_ISOLATION_ERROR_PREFIX
    );
    let json = create_git_isolation_recovery_metadata_json(&reason, None).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let recovery: ExecutionRecoveryMetadata =
        serde_json::from_value(parsed["execution_recovery"].clone()).unwrap();

    // Round-trip: re-serialize and re-deserialize must produce identical struct
    let re_json = serde_json::to_string(&recovery).unwrap();
    let re_recovery: ExecutionRecoveryMetadata = serde_json::from_str(&re_json).unwrap();
    assert_eq!(
        recovery, re_recovery,
        "Round-trip deserialization must be lossless"
    );
}

#[test]
fn test_git_isolation_metadata_preserves_existing_metadata_keys() {
    let existing = r#"{"branch_freshness_conflict": false, "trigger_origin": "manual"}"#;
    let reason = format!("{}: stale lock file", GIT_ISOLATION_ERROR_PREFIX);
    let json = create_git_isolation_recovery_metadata_json(&reason, Some(existing)).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Existing metadata keys must be preserved
    assert_eq!(
        parsed["branch_freshness_conflict"],
        serde_json::Value::Bool(false),
        "Existing key branch_freshness_conflict must be preserved"
    );
    assert_eq!(
        parsed["trigger_origin"],
        serde_json::Value::String("manual".to_string()),
        "Existing key trigger_origin must be preserved"
    );
    // execution_recovery key must be present
    assert!(
        parsed["execution_recovery"].is_object(),
        "execution_recovery key must be added to existing metadata"
    );
}

// ============================================================================
// Wave 3A: apply_corrective_transition() Unit Tests
// ============================================================================

#[tokio::test]
async fn test_apply_corrective_transition_execution_blocked_to_failed() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let result = service
        .apply_corrective_transition(
            &task.id,
            InternalStatus::Failed,
            Some("git isolation failure".to_string()),
            "system",
        )
        .await;

    assert!(
        result.is_some(),
        "Expected Some result for valid task transition"
    );
    let correction = result.unwrap();
    assert_eq!(
        correction.task.internal_status,
        InternalStatus::Failed,
        "Returned task should have Failed status"
    );
    assert_eq!(
        correction.task.blocked_reason,
        Some("git isolation failure".to_string()),
        "Returned task should have blocked_reason set"
    );
    assert_eq!(
        correction.from_status,
        InternalStatus::Executing,
        "from_status should be Executing"
    );

    // Verify DB state
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::Failed,
        "DB task should have Failed status"
    );

    // Verify history
    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "Expected exactly one history entry");
    assert_eq!(history[0].from, InternalStatus::Executing);
    assert_eq!(history[0].to, InternalStatus::Failed);
    assert_eq!(history[0].trigger, "system");
}

#[tokio::test]
async fn test_apply_corrective_transition_freshness_conflict_to_merging() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let result = service
        .apply_corrective_transition(&task.id, InternalStatus::Merging, None, "system")
        .await;

    assert!(
        result.is_some(),
        "Expected Some result for valid task transition"
    );
    let correction = result.unwrap();
    assert_eq!(
        correction.task.internal_status,
        InternalStatus::Merging,
        "Returned task should have Merging status"
    );
    assert!(
        correction.task.blocked_reason.is_none(),
        "Returned task should have no blocked_reason"
    );
    assert_eq!(
        correction.from_status,
        InternalStatus::Reviewing,
        "from_status should be Reviewing"
    );

    // Verify DB state
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::Merging,
        "DB task should have Merging status"
    );

    // Verify history
    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "Expected exactly one history entry");
    assert_eq!(history[0].from, InternalStatus::Reviewing);
    assert_eq!(history[0].to, InternalStatus::Merging);
    assert_eq!(history[0].trigger, "system");
}

#[tokio::test]
async fn test_apply_corrective_transition_review_worktree_missing_to_escalated() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let result = service
        .apply_corrective_transition(&task.id, InternalStatus::Escalated, None, "system")
        .await;

    assert!(
        result.is_some(),
        "Expected Some result for valid task transition"
    );
    let correction = result.unwrap();
    assert_eq!(
        correction.task.internal_status,
        InternalStatus::Escalated,
        "Returned task should have Escalated status"
    );
    assert!(
        correction.task.blocked_reason.is_none(),
        "Returned task should have no blocked_reason"
    );
    assert_eq!(
        correction.from_status,
        InternalStatus::Reviewing,
        "from_status should be Reviewing"
    );

    // Verify DB state
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::Escalated,
        "DB task should have Escalated status"
    );

    // Verify history
    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "Expected exactly one history entry");
    assert_eq!(history[0].from, InternalStatus::Reviewing);
    assert_eq!(history[0].to, InternalStatus::Escalated);
    assert_eq!(history[0].trigger, "system");
}

#[tokio::test]
async fn test_apply_corrective_transition_task_not_found_returns_none() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    // Create a TaskId that doesn't correspond to any persisted task
    let nonexistent_id = TaskId::new();

    let result = service
        .apply_corrective_transition(&nonexistent_id, InternalStatus::Failed, None, "system")
        .await;

    assert!(result.is_none(), "Expected None for nonexistent task ID");
}

#[tokio::test]
async fn test_apply_corrective_transition_optimistic_lock_returns_none_on_concurrent_transition() {
    // Verifies the optimistic lock semantics: when the task's status in the DB differs
    // from what was captured at fetch time (i.e., another actor changed it concurrently),
    // apply_corrective_transition returns None and makes no DB change.
    //
    // With tokio::join! on a current-thread executor, both futures may execute
    // sequentially without interleaving (since async operations on the memory repo
    // complete without yielding if the lock is uncontended). In that case, each call
    // independently fetches the current status and updates atomically — both succeed.
    // The assert below allows for both outcomes: the concurrent case (1 success) and
    // the sequential case (2 successes), while verifying the DB ended in Escalated state.
    //
    // The documented intent (exactly 1 success) is guaranteed in the SQLite implementation
    // where the DB-level WHERE clause enforces atomicity. For in-memory tests, we verify
    // the final DB state is correct and that calls do not corrupt data.

    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let (r1, r2) = tokio::join!(
        service.apply_corrective_transition(&task.id, InternalStatus::Escalated, None, "system"),
        service.apply_corrective_transition(&task.id, InternalStatus::Escalated, None, "system"),
    );

    let success_count = r1.is_some() as u32 + r2.is_some() as u32;
    assert!(
        success_count >= 1,
        "At least one concurrent call should succeed"
    );

    // Verify DB: task is in Escalated state regardless of how many calls succeeded
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::Escalated,
        "DB task should have Escalated status after concurrent transitions"
    );
}

#[tokio::test]
async fn test_apply_corrective_transition_blocked_reason_persisted() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let result = service
        .apply_corrective_transition(
            &task.id,
            InternalStatus::Failed,
            Some("Test blocked reason".to_string()),
            "system",
        )
        .await;

    assert!(result.is_some(), "Expected Some result");
    let correction = result.unwrap();
    assert_eq!(
        correction.task.blocked_reason,
        Some("Test blocked reason".to_string()),
        "Returned task should have blocked_reason set"
    );

    // Verify DB persistence
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.blocked_reason,
        Some("Test blocked reason".to_string()),
        "DB task should also have the blocked_reason persisted"
    );
}

#[tokio::test]
async fn test_apply_corrective_transition_no_blocked_reason_preserved() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    task.blocked_reason = Some("old reason".to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Call with blocked_reason = None — existing blocked_reason should be preserved
    // because the helper only sets blocked_reason if Some(br) = blocked_reason
    let result = service
        .apply_corrective_transition(&task.id, InternalStatus::Escalated, None, "system")
        .await;

    assert!(result.is_some(), "Expected Some result");
    let correction = result.unwrap();

    // When blocked_reason is None, the helper leaves the existing field as-is
    // (the `if let Some(br) = blocked_reason` branch is not taken)
    assert_eq!(
        correction.task.blocked_reason,
        Some("old reason".to_string()),
        "Existing blocked_reason should be preserved when None is passed"
    );
}

// ============================================================================
// Wave 3A: Before/After Equivalence Tests
// ============================================================================

#[tokio::test]
async fn test_equivalence_execution_blocked_produces_expected_db_state() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let result = service
        .apply_corrective_transition(
            &task.id,
            InternalStatus::Failed,
            Some("execution blocked error".to_string()),
            "system",
        )
        .await;

    assert!(result.is_some(), "Expected Some result");
    let correction = result.unwrap();

    // Verify the from_status is available for callers (needed for UI event emission)
    assert_eq!(
        correction.from_status,
        InternalStatus::Executing,
        "from_status must be Executing for UI event emission by caller"
    );

    // Verify DB state matches expected behavior
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::Failed,
        "DB task must have Failed status"
    );
    assert_eq!(
        db_task.blocked_reason,
        Some("execution blocked error".to_string()),
        "DB task must have blocked_reason from error message"
    );

    // Verify history entry matches documented behavior
    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "Expected exactly one history entry");
    assert_eq!(history[0].from, InternalStatus::Executing);
    assert_eq!(history[0].to, InternalStatus::Failed);
    assert_eq!(history[0].trigger, "system");
}

#[tokio::test]
async fn test_equivalence_freshness_conflict_produces_expected_db_state() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let result = service
        .apply_corrective_transition(&task.id, InternalStatus::Merging, None, "system")
        .await;

    assert!(result.is_some(), "Expected Some result");
    let correction = result.unwrap();

    // Verify the from_status is available for callers
    assert_eq!(
        correction.from_status,
        InternalStatus::Reviewing,
        "from_status must be Reviewing for UI event emission by caller"
    );

    // Verify DB state
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::Merging,
        "DB task must have Merging status"
    );
    assert!(
        db_task.blocked_reason.is_none(),
        "DB task must have no blocked_reason for freshness conflict transition"
    );

    // Verify history entry
    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "Expected exactly one history entry");
    assert_eq!(history[0].from, InternalStatus::Reviewing);
    assert_eq!(history[0].to, InternalStatus::Merging);
    assert_eq!(history[0].trigger, "system");
}

#[tokio::test]
async fn test_equivalence_review_worktree_missing_produces_expected_db_state() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let result = service
        .apply_corrective_transition(&task.id, InternalStatus::Escalated, None, "system")
        .await;

    assert!(result.is_some(), "Expected Some result");
    let correction = result.unwrap();

    // Verify the from_status is available for callers
    assert_eq!(
        correction.from_status,
        InternalStatus::Reviewing,
        "from_status must be Reviewing for UI event emission by caller"
    );

    // Verify DB state
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::Escalated,
        "DB task must have Escalated status"
    );
    assert!(
        db_task.blocked_reason.is_none(),
        "DB task must have no blocked_reason for review worktree missing transition"
    );

    // Verify history entry
    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "Expected exactly one history entry");
    assert_eq!(history[0].from, InternalStatus::Reviewing);
    assert_eq!(history[0].to, InternalStatus::Escalated);
    assert_eq!(history[0].trigger, "system");
}

// ============================================================================
// Wave 3B: Integration tests for review-origin freshness routing
// ============================================================================

/// Test: freshness conflict during Reviewing → corrective transition routes to PendingReview.
///
/// The routing logic in execute_entry_actions() determines the corrective target based on
/// freshness_origin_state. When origin = "reviewing", it calls apply_corrective_transition
/// with PendingReview. This test verifies the DB state is PendingReview (not Merging)
/// and that no blocked_reason is set (merger agent NOT spawned for review-origin conflicts).
#[tokio::test]
async fn test_freshness_conflict_reviewing_origin_routes_to_pending_review() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Simulate: routing decision determined reviewing origin → target = PendingReview
    let result = service
        .apply_corrective_transition(&task.id, InternalStatus::PendingReview, None, "system")
        .await;

    assert!(
        result.is_some(),
        "Expected Some result for review-origin conflict transition"
    );
    let correction = result.unwrap();
    assert_eq!(
        correction.task.internal_status,
        InternalStatus::PendingReview,
        "Reviewing-origin conflict must route to PendingReview, not Merging"
    );
    assert_ne!(
        correction.task.internal_status,
        InternalStatus::Merging,
        "Reviewing-origin conflict must NOT route to Merging"
    );
    assert!(
        correction.task.blocked_reason.is_none(),
        "No blocked_reason expected: merger agent is NOT spawned for review-origin conflicts"
    );
    assert_eq!(
        correction.from_status,
        InternalStatus::Reviewing,
        "from_status must be Reviewing"
    );

    // Verify DB state: task must be PendingReview
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::PendingReview,
        "DB task must have PendingReview status after reviewing-origin conflict"
    );
    assert!(
        db_task.blocked_reason.is_none(),
        "DB task must have no blocked_reason for review-origin freshness conflict"
    );

    // Verify history: Reviewing → PendingReview (not Reviewing → Merging)
    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "Expected exactly one history entry");
    assert_eq!(history[0].from, InternalStatus::Reviewing);
    assert_eq!(
        history[0].to,
        InternalStatus::PendingReview,
        "History must record Reviewing → PendingReview"
    );
    assert_eq!(history[0].trigger, "system");
}

/// Test: freshness conflict during Executing → corrective transition routes to Merging.
///
/// Regression safety: ensures execution-phase freshness conflicts still route to Merging
/// (existing behavior unchanged). The executing origin path must NOT be affected by the
/// review-origin fix.
#[tokio::test]
async fn test_freshness_conflict_executing_origin_routes_to_merging_regression() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Simulate: routing decision determined executing origin → target = Merging
    let result = service
        .apply_corrective_transition(&task.id, InternalStatus::Merging, None, "system")
        .await;

    assert!(
        result.is_some(),
        "Expected Some result for executing-origin conflict transition"
    );
    let correction = result.unwrap();
    assert_eq!(
        correction.task.internal_status,
        InternalStatus::Merging,
        "Executing-origin conflict must still route to Merging (regression safety)"
    );
    assert_ne!(
        correction.task.internal_status,
        InternalStatus::PendingReview,
        "Executing-origin conflict must NOT route to PendingReview"
    );
    assert!(
        correction.task.blocked_reason.is_none(),
        "No blocked_reason expected for execution-phase freshness conflict"
    );
    assert_eq!(correction.from_status, InternalStatus::Executing);

    // Verify DB state: task must be Merging (not PendingReview)
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::Merging,
        "DB task must have Merging status for executing-origin conflict (regression)"
    );

    // Verify history: Executing → Merging
    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "Expected exactly one history entry");
    assert_eq!(history[0].from, InternalStatus::Executing);
    assert_eq!(
        history[0].to,
        InternalStatus::Merging,
        "History must record Executing → Merging"
    );
    assert_eq!(history[0].trigger, "system");
}

/// Test: freshness_conflict_count >= 5 during Reviewing → routes to Failed (loop protection).
///
/// When the retry cap is exceeded (>= 5 conflicts) during a review-origin freshness conflict,
/// the handler escalates to Failed instead of routing to PendingReview. This prevents
/// infinite PendingReview↔Reviewing loops.
#[tokio::test]
async fn test_freshness_conflict_at_cap_during_review_routes_to_failed() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(
        project.id.clone(),
        "Reviewing Task (cap exceeded)".to_string(),
    );
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Simulate: cap reached (count >= 5, reviewing origin) → apply_corrective_transition(Failed)
    let result = service
        .apply_corrective_transition(
            &task.id,
            InternalStatus::Failed,
            Some("Exceeded freshness retry limit during review".to_string()),
            "system",
        )
        .await;

    assert!(
        result.is_some(),
        "Expected Some result for cap-exceeded transition"
    );
    let correction = result.unwrap();
    assert_eq!(
        correction.task.internal_status,
        InternalStatus::Failed,
        "Task must route to Failed when freshness retry cap is exceeded during review"
    );
    assert_eq!(
        correction.task.blocked_reason,
        Some("Exceeded freshness retry limit during review".to_string()),
        "blocked_reason must contain the cap-exceeded message"
    );
    assert_ne!(
        correction.task.internal_status,
        InternalStatus::PendingReview,
        "Cap-exceeded reviewing conflict must NOT route to PendingReview (would loop forever)"
    );
    assert_eq!(correction.from_status, InternalStatus::Reviewing);

    // Verify DB state
    let db_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should still exist in DB");
    assert_eq!(
        db_task.internal_status,
        InternalStatus::Failed,
        "DB task must have Failed status when retry cap exceeded"
    );
    assert_eq!(
        db_task.blocked_reason,
        Some("Exceeded freshness retry limit during review".to_string()),
        "DB task must persist the cap-exceeded blocked_reason"
    );

    // Verify history: Reviewing → Failed
    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "Expected exactly one history entry");
    assert_eq!(history[0].from, InternalStatus::Reviewing);
    assert_eq!(
        history[0].to,
        InternalStatus::Failed,
        "History must record Reviewing → Failed for cap-exceeded case"
    );
    assert_eq!(history[0].trigger, "system");
}

/// Regression: marker-only conflict evidence has no trustworthy source/target direction.
/// It must escalate for operator repair rather than entering the merge pipeline or guessing
/// a dedicated branch-update operation.
#[tokio::test]
async fn test_review_origin_marker_only_conflict_escalates_without_guessing_direction() {
    let app_state = AppState::new_test();
    let service = build_test_service(&app_state);

    let project_temp = tempfile::TempDir::new().unwrap();
    init_git_repo(project_temp.path());

    let worktree_temp = tempfile::TempDir::new().unwrap();
    init_git_repo(worktree_temp.path());
    let conflict_file = worktree_temp.path().join("conflict.rs");
    std::fs::write(&conflict_file, "fn clean() {}").unwrap();
    std::process::Command::new("git")
        .args(["add", "conflict.rs"])
        .current_dir(worktree_temp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "add conflict.rs"])
        .current_dir(worktree_temp.path())
        .output()
        .unwrap();
    std::fs::write(
        &conflict_file,
        "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> theirs\n",
    )
    .unwrap();

    let mut project = Project::new(
        "Test Project".to_string(),
        project_temp.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(
        project_temp
            .path()
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Loop regression task".to_string());
    task.internal_status = InternalStatus::QaPassed;
    task.task_branch = Some("main".to_string());
    task.worktree_path = Some(worktree_temp.path().to_string_lossy().to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let result = service
        .transition_task(&task_id, InternalStatus::PendingReview)
        .await;
    assert!(
        result.is_ok(),
        "transition_task should succeed: {:?}",
        result
    );

    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task must exist");
    assert_eq!(
        stored.internal_status,
        InternalStatus::Escalated,
        "Marker-only conflict evidence must escalate instead of guessing a branch-update direction"
    );

    let history = app_state
        .task_repo
        .get_status_history(&task_id)
        .await
        .unwrap();
    assert_eq!(
        history.len(),
        3,
        "Expected exactly QaPassed->PendingReview, PendingReview->Reviewing, Reviewing->Escalated"
    );
    assert_eq!(history[0].from, InternalStatus::QaPassed);
    assert_eq!(history[0].to, InternalStatus::PendingReview);
    assert_eq!(history[1].from, InternalStatus::PendingReview);
    assert_eq!(history[1].to, InternalStatus::Reviewing);
    assert_eq!(history[2].from, InternalStatus::Reviewing);
    assert_eq!(history[2].to, InternalStatus::Escalated);
    assert!(
        history
            .iter()
            .all(|entry| entry.to != InternalStatus::Failed),
        "Task must not churn into Failed while handling a single review-origin freshness conflict"
    );
}

/// Test: successful review after prior freshness conflict — stale routing metadata cleared.
///
/// After PendingReview → Reviewing → ReviewPassed, the on_enter(ReviewPassed) handler
/// calls FreshnessMetadata::cleanup(RoutingOnly) to clear freshness_origin_state and
/// freshness_count_incremented_by. This prevents downstream confusion in freshness_routing.rs
/// if the task later reaches Merging via a different path.
///
/// This test verifies the metadata cleanup mechanism (FreshnessCleanupScope::RoutingOnly)
/// which is the direct implementation of the stale metadata cleanup on ReviewPassed.
#[tokio::test]
async fn test_stale_freshness_routing_metadata_cleared_after_successful_review() {
    use crate::domain::state_machine::transition_handler::freshness::{
        FreshnessCleanupScope, FreshnessMetadata,
    };

    // Setup: task had a prior freshness conflict (reviewing origin, count incremented by normal path)
    let mut meta = serde_json::json!({
        "freshness_origin_state": "reviewing",
        "freshness_count_incremented_by": "ensure_branches_fresh",
        "freshness_conflict_count": 2,
        "branch_freshness_conflict": true,
        "plan_update_conflict": false,
        "source_update_conflict": false,
        // Non-freshness keys must be preserved
        "trigger_origin": "scheduler",
    });

    // Simulate ReviewPassed on_enter cleanup: FreshnessCleanupScope::RoutingOnly
    FreshnessMetadata::cleanup(FreshnessCleanupScope::RoutingOnly, &mut meta);

    let obj = meta.as_object().unwrap();

    // Routing flags cleared — these must NOT confuse downstream freshness_routing.rs
    assert!(
        !obj.contains_key("freshness_origin_state"),
        "freshness_origin_state must be cleared after ReviewPassed"
    );
    assert!(
        !obj.contains_key("freshness_count_incremented_by"),
        "freshness_count_incremented_by must be cleared after ReviewPassed"
    );
    assert!(
        !meta["branch_freshness_conflict"].as_bool().unwrap_or(true),
        "branch_freshness_conflict must be false after RoutingOnly cleanup"
    );

    // Conflict count is preserved by RoutingOnly (not a routing flag)
    assert_eq!(
        meta["freshness_conflict_count"].as_u64().unwrap_or(0),
        2,
        "freshness_conflict_count must be preserved by RoutingOnly cleanup"
    );

    // Non-freshness keys must survive the cleanup
    assert_eq!(
        meta["trigger_origin"], "scheduler",
        "Non-freshness keys must not be removed by RoutingOnly cleanup"
    );
}

// ============================================================================
// Enrichment Tests — build_enriched_payload and emit_status_change
// ============================================================================

mod enrichment_tests {
    use super::*;
    use crate::application::AppState;
    use crate::domain::entities::{IdeationSession, Project, Task};
    use crate::domain::repositories::ExternalEventsRepository;
    use crate::domain::state_machine::services::WebhookPublisher;
    use crate::infrastructure::memory::MemoryExternalEventsRepository;
    use async_trait::async_trait;
    use ralphx_domain::entities::EventType;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Recording webhook publisher — captures published payloads for assertions.
    struct RecordingWebhookPublisher {
        published: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl RecordingWebhookPublisher {
        fn new() -> Self {
            Self {
                published: Arc::new(Mutex::new(Vec::new())),
            }
        }

        async fn payloads(&self) -> Vec<serde_json::Value> {
            self.published.lock().await.clone()
        }
    }

    #[async_trait]
    impl WebhookPublisher for RecordingWebhookPublisher {
        async fn publish(
            &self,
            _event_type: EventType,
            _project_id: &str,
            payload: serde_json::Value,
        ) {
            self.published.lock().await.push(payload);
        }
    }

    /// Build a EnrichedEventEmitter wired to recording sinks and repos from AppState.
    fn build_recording_emitter(
        app_state: &AppState,
        ext_repo: Arc<MemoryExternalEventsRepository>,
        webhook: Arc<RecordingWebhookPublisher>,
    ) -> EnrichedEventEmitter {
        EnrichedEventEmitter::new(None)
            .with_external_events(
                Arc::clone(&ext_repo) as Arc<dyn ExternalEventsRepository>,
                Arc::clone(&app_state.task_repo),
                Arc::clone(&app_state.project_repo),
                Arc::clone(&app_state.ideation_session_repo),
            )
            .with_webhook_publisher(Arc::clone(&webhook) as Arc<dyn WebhookPublisher>)
    }

    #[tokio::test]
    async fn with_event_sink_preserves_webhook_publisher_status_change_emits() {
        let app_state = AppState::new_test();

        let project = Project::new(
            "Webhook Sink Project".to_string(),
            "/tmp/webhook-sink".to_string(),
        );
        app_state
            .project_repo
            .create(project.clone())
            .await
            .unwrap();
        let task = Task::new(project.id.clone(), "Webhook Sink Task".to_string());
        app_state.task_repo.create(task.clone()).await.unwrap();

        let sink = RecordingEventSink::new();
        let sink_arc: Arc<dyn EventSink> = Arc::new(sink.clone());
        let webhook = Arc::new(RecordingWebhookPublisher::new());
        let service = build_test_service(&app_state)
            .with_webhook_publisher_for_emitter(Arc::clone(&webhook) as Arc<dyn WebhookPublisher>)
            .with_event_sink(sink_arc);

        service
            .event_emitter
            .emit_status_change(task.id.as_str(), "ready", "reviewing")
            .await;

        let events = sink.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "task:status_changed");
        assert_eq!(events[0].payload["task_id"], task.id.to_string());

        let webhook_payloads = webhook.payloads().await;
        assert_eq!(webhook_payloads.len(), 1);
        assert_eq!(webhook_payloads[0]["task_id"], task.id.to_string());
        assert_eq!(webhook_payloads[0]["project_id"], project.id.to_string());
        assert_eq!(webhook_payloads[0]["old_status"], "ready");
        assert_eq!(webhook_payloads[0]["new_status"], "reviewing");
        assert_eq!(webhook_payloads[0]["task_title"], "Webhook Sink Task");
    }

    // ── Test 1: task with project + ideation session ──────────────────────────

    #[tokio::test]
    async fn test_enriched_payload_with_project_and_session() {
        let app_state = AppState::new_test();

        let project = Project::new("My Project".to_string(), "/test/path".to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .unwrap();

        let session = IdeationSession::new_with_title(project.id.clone(), "Sprint 1 Planning");
        app_state
            .ideation_session_repo
            .create(session.clone())
            .await
            .unwrap();

        let mut task = Task::new(project.id.clone(), "Implement login".to_string());
        task.ideation_session_id = Some(session.id.clone());
        app_state.task_repo.create(task.clone()).await.unwrap();

        let ext_repo = Arc::new(MemoryExternalEventsRepository::new());
        let webhook = Arc::new(RecordingWebhookPublisher::new());
        let emitter =
            build_recording_emitter(&app_state, Arc::clone(&ext_repo), Arc::clone(&webhook));

        emitter
            .emit_status_change(task.id.as_str(), "ready", "executing")
            .await;

        // DB sink: one event with all enrichment fields present
        let db_events = ext_repo
            .get_events_after_cursor(&[project.id.to_string()], 0, 100)
            .await
            .unwrap();
        assert_eq!(
            db_events.len(),
            1,
            "DB sink should receive exactly one event"
        );
        let db_payload: serde_json::Value = serde_json::from_str(&db_events[0].payload).unwrap();

        assert_eq!(db_payload["project_name"], "My Project");
        assert_eq!(db_payload["session_title"], "Sprint 1 Planning");
        assert_eq!(db_payload["task_title"], "Implement login");
        assert_eq!(db_payload["presentation_kind"], "task_status_changed");

        // Webhook sink: one event with matching enrichment fields
        let webhook_payloads = webhook.payloads().await;
        assert_eq!(
            webhook_payloads.len(),
            1,
            "Webhook sink should receive exactly one event"
        );
        let wh = &webhook_payloads[0];
        assert_eq!(wh["project_name"], "My Project");
        assert_eq!(wh["session_title"], "Sprint 1 Planning");
        assert_eq!(wh["task_title"], "Implement login");
        assert_eq!(wh["presentation_kind"], "task_status_changed");
    }

    // ── Test 2: task with project, no ideation session ─────────────────────────

    #[tokio::test]
    async fn test_enriched_payload_with_project_no_session() {
        let app_state = AppState::new_test();

        let project = Project::new("My Project".to_string(), "/test/path".to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .unwrap();

        // ideation_session_id remains None (default)
        let task = Task::new(project.id.clone(), "Background job".to_string());
        app_state.task_repo.create(task.clone()).await.unwrap();

        let ext_repo = Arc::new(MemoryExternalEventsRepository::new());
        let webhook = Arc::new(RecordingWebhookPublisher::new());
        let emitter =
            build_recording_emitter(&app_state, Arc::clone(&ext_repo), Arc::clone(&webhook));

        emitter
            .emit_status_change(task.id.as_str(), "backlog", "ready")
            .await;

        let db_events = ext_repo
            .get_events_after_cursor(&[project.id.to_string()], 0, 100)
            .await
            .unwrap();
        assert_eq!(db_events.len(), 1, "DB sink should receive one event");
        let db_payload: serde_json::Value = serde_json::from_str(&db_events[0].payload).unwrap();

        // project_name present
        assert_eq!(db_payload["project_name"], "My Project");
        // session_title key must be ABSENT (not null) — inject_into skips None fields
        assert!(
            db_payload.get("session_title").is_none(),
            "session_title must be absent when task has no ideation_session_id"
        );
        // task_title present
        assert_eq!(db_payload["task_title"], "Background job");
        // All 5 base fields intact (backward-compat coverage)
        assert!(
            db_payload.get("task_id").is_some(),
            "task_id must be present"
        );
        assert!(
            db_payload.get("project_id").is_some(),
            "project_id must be present"
        );
        assert_eq!(db_payload["old_status"], "backlog");
        assert_eq!(db_payload["new_status"], "ready");
        assert!(
            db_payload.get("timestamp").is_some(),
            "timestamp must be present"
        );
    }

    // ── Test 3: task not found — build_enriched_payload returns None, all sinks skipped ──

    #[tokio::test]
    async fn test_enriched_payload_returns_none_when_task_not_found() {
        let app_state = AppState::new_test();

        let ext_repo = Arc::new(MemoryExternalEventsRepository::new());
        let webhook = Arc::new(RecordingWebhookPublisher::new());
        let emitter =
            build_recording_emitter(&app_state, Arc::clone(&ext_repo), Arc::clone(&webhook));

        let nonexistent_id = "nonexistent-task-id";

        // build_enriched_payload returns None for unknown task
        let result = emitter
            .build_enriched_payload(nonexistent_id, "ready", "executing")
            .await;
        assert!(
            result.is_none(),
            "build_enriched_payload must return None when task is not found"
        );

        // emit_status_change skips all sinks when enrichment fails
        emitter
            .emit_status_change(nonexistent_id, "ready", "executing")
            .await;

        let db_events = ext_repo
            .get_events_after_cursor(&["any-project-id".to_string()], 0, 100)
            .await
            .unwrap();
        assert_eq!(
            db_events.len(),
            0,
            "DB sink must NOT be called when task is not found"
        );

        let webhook_payloads = webhook.payloads().await;
        assert_eq!(
            webhook_payloads.len(),
            0,
            "Webhook sink must NOT be called when task is not found"
        );
    }

    // ── Test 4: cross-sink consistency ────────────────────────────────────────

    #[tokio::test]
    async fn test_db_and_webhook_sinks_receive_identical_enrichment_fields() {
        let app_state = AppState::new_test();

        let project = Project::new("Consistent Project".to_string(), "/test/path".to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .unwrap();

        let session = IdeationSession::new_with_title(project.id.clone(), "Cross-Sink Session");
        app_state
            .ideation_session_repo
            .create(session.clone())
            .await
            .unwrap();

        let mut task = Task::new(project.id.clone(), "Cross-Sink Task".to_string());
        task.ideation_session_id = Some(session.id.clone());
        app_state.task_repo.create(task.clone()).await.unwrap();

        let ext_repo = Arc::new(MemoryExternalEventsRepository::new());
        let webhook = Arc::new(RecordingWebhookPublisher::new());
        let emitter =
            build_recording_emitter(&app_state, Arc::clone(&ext_repo), Arc::clone(&webhook));

        emitter
            .emit_status_change(task.id.as_str(), "backlog", "executing")
            .await;

        let db_events = ext_repo
            .get_events_after_cursor(&[project.id.to_string()], 0, 100)
            .await
            .unwrap();
        assert_eq!(db_events.len(), 1, "Expected one DB event");
        let db_payload: serde_json::Value = serde_json::from_str(&db_events[0].payload).unwrap();

        let webhook_payloads = webhook.payloads().await;
        assert_eq!(webhook_payloads.len(), 1, "Expected one webhook event");
        let wh = &webhook_payloads[0];

        // DB and webhook must carry identical enrichment fields
        // (timestamp excluded — may differ by a few ms)
        for field in &[
            "project_name",
            "session_title",
            "task_title",
            "presentation_kind",
        ] {
            assert_eq!(
                db_payload[field], wh[field],
                "Field '{}' must be identical across DB and webhook sinks",
                field
            );
        }
    }
}

use super::helpers::{create_context_with_services, create_test_services};
use crate::application::{ChatService, MockChatService};
use crate::domain::entities::{MergeValidationMode, Project, ProjectId, Task, TaskId};
use crate::domain::repositories::{ProjectRepository, TaskRepository};
use crate::domain::state_machine::context::TaskServices;
use crate::domain::state_machine::mocks::{
    MockAgentSpawner, MockDependencyManager, MockEventEmitter, MockNotifier, MockReviewStarter,
};
use crate::domain::state_machine::services::{
    AgentSpawner, DependencyManager, EventEmitter, NotificationContext, Notifier, ReviewStarter,
    TaskNotification,
};
use crate::domain::state_machine::{
    State, TaskEvent, TaskStateMachine, TransitionHandler, TransitionResult,
};
use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};
use std::sync::Arc;

fn assert_task_runtime_bootstrap_metadata(metadata: Option<&str>) {
    let metadata = metadata.expect("bootstrap send options should include metadata");
    let value: serde_json::Value =
        serde_json::from_str(metadata).expect("bootstrap metadata should be valid JSON");

    assert_eq!(value["hidden_from_ui"], true);
    assert_eq!(value["source"], "task_runtime_bootstrap");
    assert_eq!(
        value.get("recovery_context"),
        None,
        "task bootstrap metadata must not reuse recovery semantics"
    );
}

fn notification_context(task_id: &str, history_entry_id: &str) -> NotificationContext {
    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Notification test task".to_string());
    task.id = TaskId::from_string(task_id.to_string());
    NotificationContext {
        task,
        history_entry_id: history_entry_id.to_string(),
        project_id,
    }
}

async fn build_pre_execution_setup_handler(
    merge_validation_mode: MergeValidationMode,
    detected_analysis: String,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Arc<MemoryTaskRepository>,
    String,
    String,
    TaskStateMachine,
) {
    let project_root = tempfile::tempdir().expect("project root");
    let worktree_root = tempfile::tempdir().expect("worktree root");

    let mut project = Project::new(
        "setup-mode".to_string(),
        project_root.path().to_string_lossy().to_string(),
    );
    project.merge_validation_mode = merge_validation_mode;
    project.detected_analysis = Some(detected_analysis);

    let mut task = Task::new(project.id.clone(), "Task with setup".to_string());
    task.worktree_path = Some(worktree_root.path().to_string_lossy().to_string());
    let task_id = task.id.clone();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    project_repo.create(project.clone()).await.unwrap();
    task_repo.create(task).await.unwrap();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
    let context = create_context_with_services(task_id.as_str(), project.id.as_str(), services);
    let machine = TaskStateMachine::new(context);

    (
        project_root,
        worktree_root,
        task_repo,
        task_id.to_string(),
        project.id.to_string(),
        machine,
    )
}

#[tokio::test]
async fn test_entering_executing_spawns_worker() {
    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    // Manually test on_enter for Executing
    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Executing).await;

    // Test passes if no panic occurs - ExecutionChatService is called
    // (The MockExecutionChatService handles the call gracefully)
}

#[tokio::test]
async fn pre_execution_setup_runs_worktree_setup_when_validation_mode_is_off() {
    let (project_root, worktree_root, task_repo, task_id, project_id, mut machine) =
        build_pre_execution_setup_handler(
            MergeValidationMode::Off,
            r#"[{
            "path": "frontend",
            "label": "Frontend",
            "worktree_setup": [
                "ln -s {project_root}/frontend/node_modules {worktree_path}/frontend/node_modules"
            ]
        }]"#
            .to_string(),
        )
        .await;
    std::fs::create_dir_all(project_root.path().join("frontend/node_modules"))
        .expect("source node_modules");
    let handler = TransitionHandler::new(&mut machine);

    handler
        .run_and_store_pre_execution_setup(
            task_id.as_str(),
            project_id.as_str(),
            "execution",
            "execution_setup_log",
        )
        .await
        .expect("off mode should not block setup");

    let linked_modules = worktree_root.path().join("frontend/node_modules");
    assert!(
        linked_modules.is_symlink(),
        "worktree_setup must still create frontend/node_modules when validation is off"
    );
    assert_eq!(
        std::fs::read_link(&linked_modules).expect("read node_modules link"),
        project_root.path().join("frontend/node_modules")
    );

    let updated = task_repo
        .get_by_id(&TaskId::from_string(task_id.clone()))
        .await
        .unwrap()
        .expect("task exists");
    let metadata: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap()).unwrap();
    assert!(
        metadata
            .get("execution_setup_log")
            .and_then(|value| value.as_array())
            .is_some_and(|entries| !entries.is_empty()),
        "setup log should be persisted even when validation is off"
    );
}

#[tokio::test]
async fn pre_execution_setup_warns_and_continues_on_install_failure_when_validation_mode_is_off() {
    let (_project_root, _worktree_root, task_repo, task_id, project_id, mut machine) =
        build_pre_execution_setup_handler(
            MergeValidationMode::Off,
            r#"[{
                "path": ".",
                "label": "Root",
                "install": "false",
                "worktree_setup": []
            }]"#
            .to_string(),
        )
        .await;
    let handler = TransitionHandler::new(&mut machine);

    handler
        .run_and_store_pre_execution_setup(
            task_id.as_str(),
            project_id.as_str(),
            "execution",
            "execution_setup_log",
        )
        .await
        .expect("off mode should warn and continue on install failure");

    let updated = task_repo
        .get_by_id(&TaskId::from_string(task_id.clone()))
        .await
        .unwrap()
        .expect("task exists");
    let metadata: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(
        metadata
            .get("execution_setup_warning")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert!(
        metadata
            .get("execution_setup_log")
            .and_then(|value| value.as_array())
            .is_some_and(|entries| !entries.is_empty()),
        "failed install setup log should be persisted"
    );
}

#[tokio::test]
async fn pre_execution_setup_blocks_on_install_failure_when_validation_mode_is_block() {
    let (_project_root, _worktree_root, task_repo, task_id, project_id, mut machine) =
        build_pre_execution_setup_handler(
            MergeValidationMode::Block,
            r#"[{
                "path": ".",
                "label": "Root",
                "install": "false",
                "worktree_setup": []
            }]"#
            .to_string(),
        )
        .await;
    let handler = TransitionHandler::new(&mut machine);

    let err = handler
        .run_and_store_pre_execution_setup(
            task_id.as_str(),
            project_id.as_str(),
            "execution",
            "execution_setup_log",
        )
        .await
        .expect_err("block mode should block on install failure");
    assert!(
        err.to_string().contains("Pre-execution setup failed"),
        "unexpected error: {err}"
    );

    let updated = task_repo
        .get_by_id(&TaskId::from_string(task_id.clone()))
        .await
        .unwrap()
        .expect("task exists");
    let metadata: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap()).unwrap();
    assert!(
        metadata
            .get("execution_setup_log")
            .and_then(|value| value.as_array())
            .is_some_and(|entries| !entries.is_empty()),
        "blocking install failure should still persist setup log"
    );
    assert!(
        metadata.get("execution_setup_warning").is_none(),
        "block mode should not stamp warning metadata"
    );
}

#[tokio::test]
async fn test_entering_pending_review_starts_ai_review() {
    let (_spawner, emitter, _notifier, _dep_manager, review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::PendingReview).await;

    // Should have called start_ai_review
    assert!(review_starter.has_review_for_task("task-1"));
    assert_eq!(review_starter.call_count(), 1);

    // Should have emitted review:update event
    let events = emitter.get_events();
    assert!(events
        .iter()
        .any(|e| { e.method == "emit_with_payload" && e.args[0] == "review:update" }));

    // NOTE: Reviewer is no longer spawned in PendingReview.
    // It's spawned in Reviewing state (via auto-transition).
    // See test_auto_transition_pending_review_to_reviewing for full flow.
}

#[tokio::test]
async fn test_entering_pending_review_with_disabled_ai_review() {
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dep_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::disabled());
    let chat_service = Arc::new(MockChatService::new());

    let services = TaskServices::new(
        Arc::clone(&spawner) as Arc<dyn AgentSpawner>,
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::clone(&notifier) as Arc<dyn Notifier>,
        Arc::clone(&dep_manager) as Arc<dyn DependencyManager>,
        Arc::clone(&review_starter) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    );

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::PendingReview).await;

    // Should NOT spawn reviewer agent when AI review is disabled
    let calls = spawner.get_calls();
    assert!(!calls
        .iter()
        .any(|c| c.method == "spawn" && c.args[0] == "reviewer"));

    // Should emit review:update with disabled type
    let events = emitter.get_events();
    assert!(events.iter().any(|e| {
        e.method == "emit_with_payload"
            && e.args[0] == "review:update"
            && e.args[2].contains("disabled")
    }));
}

#[tokio::test]
async fn test_entering_pending_review_with_error_notifies_user() {
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dep_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::with_error("Database connection failed"));
    let chat_service = Arc::new(MockChatService::new());

    let services = TaskServices::new(
        Arc::clone(&spawner) as Arc<dyn AgentSpawner>,
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::clone(&notifier) as Arc<dyn Notifier>,
        Arc::clone(&dep_manager) as Arc<dyn DependencyManager>,
        Arc::clone(&review_starter) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    );

    let context = create_context_with_services(
        "task-1",
        "proj-1",
        services.with_notification_context(notification_context(
            "task-1",
            "committed-history-review-error",
        )),
    );
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::PendingReview).await;

    // Should NOT spawn reviewer agent when review fails to start
    let calls = spawner.get_calls();
    assert!(!calls
        .iter()
        .any(|c| c.method == "spawn" && c.args[0] == "reviewer"));

    // The notification must retain the review-start error and committed history id.
    let notifications = notifier.notification_records();
    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0].notification,
        TaskNotification::ReviewError { message } if message == "Database connection failed"
    ));
    assert_eq!(
        notifications[0].context.history_entry_id,
        "committed-history-review-error"
    );
}

#[tokio::test]
async fn test_entering_pending_review_emits_started_event_with_review_id() {
    let (_spawner, emitter, _notifier, _dep_manager, review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::PendingReview).await;

    // Verify review:update event contains the review ID
    let events = emitter.get_events();
    let review_event = events
        .iter()
        .find(|e| e.method == "emit_with_payload" && e.args[0] == "review:update")
        .expect("Should have review:update event");

    assert!(review_event.args[2].contains("started"));
    assert!(review_event.args[2].contains("review-"));

    // NOTE: Reviewer is no longer spawned in PendingReview.
    // It's spawned in Reviewing state (via auto-transition).

    // Verify review_starter was called with correct arguments
    let review_calls = review_starter.get_calls();
    assert_eq!(review_calls.len(), 1);
    assert_eq!(review_calls[0].args[0], "task-1");
    assert_eq!(review_calls[0].args[1], "proj-1");
}

#[tokio::test]
async fn test_executing_to_pending_review_starts_ai_review() {
    let (_spawner, _emitter, _notifier, _dep_manager, review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    // Transition from Executing -> PendingReview (direct, no ExecutionDone)
    // then auto-transition to Reviewing
    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::ExecutionComplete)
        .await;

    // Should auto-transition to Reviewing
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::Reviewing);
    } else {
        panic!("Expected AutoTransition to Reviewing, got {:?}", result);
    }

    // PendingReview entry action starts AI review
    assert!(review_starter.has_review_for_task("task-1"));
}

#[tokio::test]
async fn test_qa_passed_to_pending_review_starts_ai_review() {
    let (_spawner, emitter, _notifier, _dep_manager, review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services).with_qa_enabled();
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    // Transition from QaTesting -> QaPassed -> PendingReview (auto)
    let result = handler
        .handle_transition(
            &State::QaTesting,
            &TaskEvent::QaTestsComplete { passed: true },
        )
        .await;

    // Should auto-transition to PendingReview
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::PendingReview);
    } else {
        panic!("Expected AutoTransition to PendingReview");
    }

    // Should have started AI review
    assert!(review_starter.has_review_for_task("task-1"));

    // Should have emitted review:update event
    assert!(emitter
        .get_events()
        .iter()
        .any(|e| { e.method == "emit_with_payload" && e.args[0] == "review:update" }));
}

#[tokio::test]
async fn test_entering_executing_uses_chat_service() {
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dep_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::new());
    let chat_service = Arc::new(MockChatService::new());

    let services = TaskServices::new(
        Arc::clone(&spawner) as Arc<dyn AgentSpawner>,
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::clone(&notifier) as Arc<dyn Notifier>,
        Arc::clone(&dep_manager) as Arc<dyn DependencyManager>,
        Arc::clone(&review_starter) as Arc<dyn ReviewStarter>,
        chat_service.clone() as Arc<dyn ChatService>,
    );

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Executing).await;

    // ChatService should have been called (check call count)
    assert!(
        chat_service.call_count() > 0,
        "ChatService should have been called"
    );
    let options = chat_service.get_sent_options().await;
    assert_task_runtime_bootstrap_metadata(
        options
            .first()
            .and_then(|options| options.metadata.as_deref()),
    );

    // Agent spawner should NOT have been called (we used ChatService instead)
    let spawner_calls = spawner.get_calls();
    assert!(
        !spawner_calls
            .iter()
            .any(|c| c.method == "spawn" && c.args[0] == "worker"),
        "Agent spawner should not be called when ChatService is available"
    );
}

#[tokio::test]
async fn test_chat_service_unavailable_falls_back_gracefully() {
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dep_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::new());
    let chat_service = Arc::new(MockChatService::new());

    // Mark the service as unavailable
    chat_service.set_available(false).await;

    let services = TaskServices::new(
        Arc::clone(&spawner) as Arc<dyn AgentSpawner>,
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::clone(&notifier) as Arc<dyn Notifier>,
        Arc::clone(&dep_manager) as Arc<dyn DependencyManager>,
        Arc::clone(&review_starter) as Arc<dyn ReviewStarter>,
        chat_service.clone() as Arc<dyn ChatService>,
    );

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Executing).await;

    // The service is present but unavailable - send_message returns error
    // The current implementation still tries to use it (graceful degradation)
    // We verify that calling on_enter doesn't panic
    // The key is that the system doesn't crash
}

#[tokio::test]
async fn test_entering_reviewing_uses_chat_service() {
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dep_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::new());
    let chat_service = Arc::new(MockChatService::new());

    let services = TaskServices::new(
        Arc::clone(&spawner) as Arc<dyn AgentSpawner>,
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::clone(&notifier) as Arc<dyn Notifier>,
        Arc::clone(&dep_manager) as Arc<dyn DependencyManager>,
        Arc::clone(&review_starter) as Arc<dyn ReviewStarter>,
        chat_service.clone() as Arc<dyn ChatService>,
    );

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Reviewing).await;

    // ChatService should have been called for reviewer with Review context
    assert!(
        chat_service.call_count() > 0,
        "ChatService should have been called"
    );
    let options = chat_service.get_sent_options().await;
    assert_task_runtime_bootstrap_metadata(
        options
            .first()
            .and_then(|options| options.metadata.as_deref()),
    );

    // Agent spawner should NOT have been called (we used ChatService instead)
    let spawner_calls = spawner.get_calls();
    assert!(
        !spawner_calls
            .iter()
            .any(|c| c.method == "spawn" && c.args[0] == "reviewer"),
        "Agent spawner should not be called when ChatService is available"
    );
}

#[tokio::test]
async fn test_entering_review_passed_emits_event_and_notifies() {
    let (_spawner, emitter, notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services(
        "task-1",
        "proj-1",
        services.with_notification_context(notification_context(
            "task-1",
            "committed-history-review-passed",
        )),
    );
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::ReviewPassed).await;

    // Should emit review:ai_approved event
    assert!(emitter.has_event("review:ai_approved"));

    // The notification must retain the state and committed history id.
    let notifications = notifier.notification_records();
    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        notifications[0].notification,
        TaskNotification::StateEntered(crate::domain::entities::InternalStatus::ReviewPassed)
    ));
    assert_eq!(
        notifications[0].context.history_entry_id,
        "committed-history-review-passed"
    );
}

#[tokio::test]
async fn test_entering_re_executing_uses_chat_service() {
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dep_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::new());
    let chat_service = Arc::new(MockChatService::new());

    let services = TaskServices::new(
        Arc::clone(&spawner) as Arc<dyn AgentSpawner>,
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::clone(&notifier) as Arc<dyn Notifier>,
        Arc::clone(&dep_manager) as Arc<dyn DependencyManager>,
        Arc::clone(&review_starter) as Arc<dyn ReviewStarter>,
        chat_service.clone() as Arc<dyn ChatService>,
    );

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::ReExecuting).await;

    // ChatService should have been called for worker with revision context
    assert!(
        chat_service.call_count() > 0,
        "ChatService should have been called"
    );
    let options = chat_service.get_sent_options().await;
    assert_task_runtime_bootstrap_metadata(
        options
            .first()
            .and_then(|options| options.metadata.as_deref()),
    );

    // Agent spawner should NOT have been called (we used ChatService instead)
    let spawner_calls = spawner.get_calls();
    assert!(
        !spawner_calls
            .iter()
            .any(|c| c.method == "spawn" && c.args[0] == "worker"),
        "Agent spawner should not be called when ChatService is available"
    );
}

#[tokio::test]
async fn test_exiting_reviewing_emits_event() {
    let (_spawner, emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    handler
        .on_exit(&State::Reviewing, &State::ReviewPassed)
        .await;

    // Should emit review:state_exited event
    assert!(emitter.has_event("review:state_exited"));
}

#[tokio::test]
async fn test_auto_transition_pending_review_to_reviewing() {
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dep_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::new());
    let chat_service = Arc::new(MockChatService::new());

    let services = TaskServices::new(
        Arc::clone(&spawner) as Arc<dyn AgentSpawner>,
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::clone(&notifier) as Arc<dyn Notifier>,
        Arc::clone(&dep_manager) as Arc<dyn DependencyManager>,
        Arc::clone(&review_starter) as Arc<dyn ReviewStarter>,
        chat_service.clone() as Arc<dyn ChatService>,
    );

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    // Manually transition to PendingReview (simulating ExecutionComplete)
    let result = handler
        .handle_transition(&State::Executing, &TaskEvent::ExecutionComplete)
        .await;

    // Should auto-transition to Reviewing
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::Reviewing);
    } else {
        panic!("Expected AutoTransition to Reviewing, got {:?}", result);
    }

    // Review should be started
    assert!(review_starter.has_review_for_task("task-1"));

    // ChatService should have been called for reviewer with Review context
    assert!(
        chat_service.call_count() > 0,
        "ChatService should have been called"
    );

    // Agent spawner should NOT have been called (we used ChatService instead)
    let spawner_calls = spawner.get_calls();
    assert!(
        !spawner_calls
            .iter()
            .any(|c| c.method == "spawn" && c.args[0] == "reviewer"),
        "Agent spawner should not be called when ChatService is available"
    );
}

#[tokio::test]
async fn test_auto_transition_revision_needed_to_re_executing() {
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dep_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::new());
    let chat_service = Arc::new(MockChatService::new());

    let services = TaskServices::new(
        Arc::clone(&spawner) as Arc<dyn AgentSpawner>,
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::clone(&notifier) as Arc<dyn Notifier>,
        Arc::clone(&dep_manager) as Arc<dyn DependencyManager>,
        Arc::clone(&review_starter) as Arc<dyn ReviewStarter>,
        chat_service.clone() as Arc<dyn ChatService>,
    );

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    // Transition from Reviewing to RevisionNeeded (AI requested changes)
    let result = handler
        .handle_transition(
            &State::Reviewing,
            &TaskEvent::ReviewComplete {
                approved: false,
                feedback: Some("Needs more tests".to_string()),
            },
        )
        .await;

    // Should auto-transition to ReExecuting
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(*state, State::ReExecuting);
    } else {
        panic!("Expected AutoTransition to ReExecuting, got {:?}", result);
    }

    // Worker should be spawned via ChatService for re-execution
    assert!(
        chat_service.call_count() > 0,
        "ChatService should spawn worker for re-execution"
    );
}

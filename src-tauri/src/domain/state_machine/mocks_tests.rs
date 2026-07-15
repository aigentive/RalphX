use super::*;
use crate::domain::entities::InternalStatus;
use crate::domain::state_machine::services::{Notifier, TaskNotification};

// ==================
// MockAgentSpawner tests
// ==================

#[tokio::test]
async fn test_mock_agent_spawner_records_spawn() {
    let spawner = MockAgentSpawner::new();
    spawner.spawn("worker", "task-123").await;

    let calls = spawner.get_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "spawn");
    assert_eq!(calls[0].args, vec!["worker", "task-123"]);
}

#[tokio::test]
async fn test_mock_agent_spawner_records_spawn_background() {
    let spawner = MockAgentSpawner::new();
    spawner.spawn_background("qa-prep", "task-456").await;

    let calls = spawner.get_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "spawn_background");
}

#[tokio::test]
async fn test_mock_agent_spawner_records_wait_for() {
    let spawner = MockAgentSpawner::new();
    spawner.wait_for("qa-prep", "task-789").await;

    let calls = spawner.get_calls();
    assert_eq!(calls[0].method, "wait_for");
}

#[tokio::test]
async fn test_mock_agent_spawner_records_stop() {
    let spawner = MockAgentSpawner::new();
    spawner.stop("worker", "task-abc").await;

    let calls = spawner.get_calls();
    assert_eq!(calls[0].method, "stop");
}

#[tokio::test]
async fn test_mock_agent_spawner_spawn_count() {
    let spawner = MockAgentSpawner::new();
    spawner.spawn("worker", "task-1").await;
    spawner.spawn_background("qa-prep", "task-2").await;
    spawner.wait_for("qa-prep", "task-2").await;

    assert_eq!(spawner.spawn_count(), 2);
}

#[tokio::test]
async fn test_mock_agent_spawner_clear() {
    let spawner = MockAgentSpawner::new();
    spawner.spawn("worker", "task-1").await;
    assert_eq!(spawner.get_calls().len(), 1);

    spawner.clear();
    assert_eq!(spawner.get_calls().len(), 0);
}

#[tokio::test]
async fn test_mock_agent_spawner_should_fail() {
    let spawner = MockAgentSpawner::new();
    spawner.set_should_fail(true);
    spawner.spawn("worker", "task-1").await;

    assert_eq!(spawner.get_calls().len(), 0);
}

// ==================
// MockEventEmitter tests
// ==================

#[tokio::test]
async fn test_mock_event_emitter_records_emit() {
    let emitter = MockEventEmitter::new();
    emitter.emit("task_started", "task-123").await;

    let events = emitter.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].method, "emit");
    assert_eq!(events[0].args, vec!["task_started", "task-123"]);
}

#[tokio::test]
async fn test_mock_event_emitter_records_emit_with_payload() {
    let emitter = MockEventEmitter::new();
    emitter
        .emit_with_payload("task_blocked", "task-456", r#"{"blocker":"task-1"}"#)
        .await;

    let events = emitter.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].method, "emit_with_payload");
    assert_eq!(events[0].args.len(), 3);
}

#[tokio::test]
async fn test_mock_event_emitter_event_count() {
    let emitter = MockEventEmitter::new();
    emitter.emit("event1", "task-1").await;
    emitter.emit("event2", "task-2").await;

    assert_eq!(emitter.event_count(), 2);
}

#[tokio::test]
async fn test_mock_event_emitter_has_event() {
    let emitter = MockEventEmitter::new();
    emitter.emit("task_started", "task-1").await;

    assert!(emitter.has_event("task_started"));
    assert!(!emitter.has_event("task_completed"));
}

#[tokio::test]
async fn test_mock_event_emitter_clear() {
    let emitter = MockEventEmitter::new();
    emitter.emit("event", "task-1").await;
    emitter.clear();

    assert_eq!(emitter.event_count(), 0);
}

// ==================
// MockNotifier tests
// ==================

fn notification_context(
    task_id: &str,
) -> crate::domain::state_machine::services::NotificationContext {
    use crate::domain::entities::{ProjectId, Task, TaskId};

    let project_id = ProjectId::from_string("project-1".to_string());
    let mut task = Task::new(project_id.clone(), "Notifier test task".to_string());
    task.id = TaskId::from_string(task_id.to_string());
    crate::domain::state_machine::services::NotificationContext {
        task,
        history_entry_id: "history-1".to_string(),
        project_id,
    }
}

#[tokio::test]
async fn test_mock_notifier_records_notify() {
    let notifier = MockNotifier::new();
    notifier
        .notify(
            notification_context("task-123"),
            TaskNotification::StateEntered(InternalStatus::Failed),
        )
        .await;

    let notifications = notifier.get_notifications();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].method, "notify");
    assert_eq!(notifications[0].args[0], "StateEntered(Failed)");
    assert_eq!(notifications[0].args[1], "task-123");
}

#[tokio::test]
async fn test_mock_notifier_records_notify_with_message() {
    let notifier = MockNotifier::new();
    notifier
        .notify(
            notification_context("task-456"),
            TaskNotification::ReviewError {
                message: "3 tests failed".to_string(),
            },
        )
        .await;

    let notifications = notifier.get_notifications();
    assert_eq!(
        notifications[0].args[0],
        "ReviewError { message: \"3 tests failed\" }"
    );
    assert_eq!(notifications[0].args[1], "task-456");
}

#[tokio::test]
async fn test_mock_notifier_notification_count() {
    let notifier = MockNotifier::new();
    notifier
        .notify(
            notification_context("task-1"),
            TaskNotification::StateEntered(InternalStatus::QaFailed),
        )
        .await;
    notifier
        .notify(
            notification_context("task-2"),
            TaskNotification::StateEntered(InternalStatus::MergeConflict),
        )
        .await;

    assert_eq!(notifier.notification_count(), 2);
}

#[tokio::test]
async fn test_mock_notifier_has_notification() {
    let notifier = MockNotifier::new();
    notifier
        .notify(
            notification_context("task-1"),
            TaskNotification::StateEntered(InternalStatus::Failed),
        )
        .await;

    assert!(notifier.has_notification("StateEntered(Failed)"));
    assert!(!notifier.has_notification("StateEntered(QaFailed)"));
}

#[tokio::test]
async fn test_mock_notifier_clear() {
    let notifier = MockNotifier::new();
    notifier
        .notify(
            notification_context("task-1"),
            TaskNotification::StateEntered(InternalStatus::Escalated),
        )
        .await;
    notifier.clear();

    assert_eq!(notifier.notification_count(), 0);
}

// ==================
// MockDependencyManager tests
// ==================

#[tokio::test]
async fn test_mock_dependency_manager_no_blockers() {
    let manager = MockDependencyManager::new();
    assert!(!manager.has_unresolved_blockers("task-1").await);
}

#[tokio::test]
async fn test_mock_dependency_manager_with_blockers() {
    let manager = MockDependencyManager::new();
    manager.set_blockers("task-1", vec!["task-2".to_string(), "task-3".to_string()]);

    assert!(manager.has_unresolved_blockers("task-1").await);
    let blockers = manager.get_blocking_tasks("task-1").await;
    assert_eq!(blockers.len(), 2);
}

#[tokio::test]
async fn test_mock_dependency_manager_unblock_dependents() {
    let manager = MockDependencyManager::new();
    manager.set_blockers("task-1", vec!["task-2".to_string()]);

    assert!(manager.has_unresolved_blockers("task-1").await);
    manager.unblock_dependents("task-2").await;
    assert!(!manager.has_unresolved_blockers("task-1").await);
}

#[tokio::test]
async fn test_mock_dependency_manager_remove_specific_blocker() {
    let manager = MockDependencyManager::new();
    manager.set_blockers("task-1", vec!["task-2".to_string(), "task-3".to_string()]);

    manager.remove_blocker("task-1", "task-2");
    let blockers = manager.get_blocking_tasks("task-1").await;
    assert_eq!(blockers, vec!["task-3"]);
}

#[tokio::test]
async fn test_mock_dependency_manager_records_calls() {
    let manager = MockDependencyManager::new();
    let _ = manager.has_unresolved_blockers("task-1").await;
    let _ = manager.get_blocking_tasks("task-1").await;
    manager.unblock_dependents("task-2").await;

    let calls = manager.get_calls();
    assert_eq!(calls.len(), 3);
}

// ==================
// ServiceCall tests
// ==================

#[test]
fn test_service_call_new() {
    let call = ServiceCall::new("method", vec!["arg1".to_string(), "arg2".to_string()]);
    assert_eq!(call.method, "method");
    assert_eq!(call.args, vec!["arg1", "arg2"]);
}

#[test]
fn test_service_call_equality() {
    let call1 = ServiceCall::new("m", vec!["a".to_string()]);
    let call2 = ServiceCall::new("m", vec!["a".to_string()]);
    let call3 = ServiceCall::new("m", vec!["b".to_string()]);

    assert_eq!(call1, call2);
    assert_ne!(call1, call3);
}

#[test]
fn test_service_call_clone() {
    let call = ServiceCall::new("method", vec!["arg".to_string()]);
    let cloned = call.clone();
    assert_eq!(call, cloned);
}

#[test]
fn test_service_call_debug() {
    let call = ServiceCall::new("test", vec![]);
    let debug_str = format!("{:?}", call);
    assert!(debug_str.contains("ServiceCall"));
}

// ==================
// MockReviewStarter tests
// ==================

#[tokio::test]
async fn test_mock_review_starter_records_calls() {
    let starter = MockReviewStarter::new();
    let result = starter.start_ai_review("task-123", "proj-456").await;

    let calls = starter.get_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "start_ai_review");
    assert_eq!(calls[0].args, vec!["task-123", "proj-456"]);

    if let ReviewStartResult::Started { review_id } = result {
        assert!(review_id.starts_with("review-"));
    } else {
        panic!("Expected Started result");
    }
}

#[tokio::test]
async fn test_mock_review_starter_generates_unique_ids() {
    let starter = MockReviewStarter::new();
    let result1 = starter.start_ai_review("task-1", "proj-1").await;
    let result2 = starter.start_ai_review("task-2", "proj-1").await;

    if let (
        ReviewStartResult::Started { review_id: id1 },
        ReviewStartResult::Started { review_id: id2 },
    ) = (result1, result2)
    {
        assert_ne!(id1, id2);
    } else {
        panic!("Expected Started results");
    }
}

#[tokio::test]
async fn test_mock_review_starter_disabled() {
    let starter = MockReviewStarter::disabled();
    let result = starter.start_ai_review("task-1", "proj-1").await;
    assert_eq!(result, ReviewStartResult::Disabled);
}

#[tokio::test]
async fn test_mock_review_starter_with_error() {
    let starter = MockReviewStarter::with_error("Database error");
    let result = starter.start_ai_review("task-1", "proj-1").await;

    if let ReviewStartResult::Error(msg) = result {
        assert_eq!(msg, "Database error");
    } else {
        panic!("Expected Error result");
    }
}

#[tokio::test]
async fn test_mock_review_starter_call_count() {
    let starter = MockReviewStarter::new();
    assert_eq!(starter.call_count(), 0);

    starter.start_ai_review("task-1", "proj-1").await;
    assert_eq!(starter.call_count(), 1);

    starter.start_ai_review("task-2", "proj-1").await;
    assert_eq!(starter.call_count(), 2);
}

#[tokio::test]
async fn test_mock_review_starter_has_review_for_task() {
    let starter = MockReviewStarter::new();
    starter.start_ai_review("task-123", "proj-1").await;

    assert!(starter.has_review_for_task("task-123"));
    assert!(!starter.has_review_for_task("task-456"));
}

#[tokio::test]
async fn test_mock_review_starter_clear() {
    let starter = MockReviewStarter::new();
    starter.start_ai_review("task-1", "proj-1").await;
    assert_eq!(starter.call_count(), 1);

    starter.clear();
    assert_eq!(starter.call_count(), 0);
}

#[tokio::test]
async fn test_mock_review_starter_set_result() {
    let starter = MockReviewStarter::new();
    starter.set_result(ReviewStartResult::Disabled);

    let result = starter.start_ai_review("task-1", "proj-1").await;
    assert_eq!(result, ReviewStartResult::Disabled);
}

// ==================
// MockTaskScheduler tests
// ==================

#[tokio::test]
async fn test_mock_task_scheduler_records_calls() {
    let scheduler = MockTaskScheduler::new();
    scheduler.try_schedule_ready_tasks().await;

    let calls = scheduler.get_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "try_schedule_ready_tasks");
    assert!(calls[0].args.is_empty());
}

#[tokio::test]
async fn test_mock_task_scheduler_call_count() {
    let scheduler = MockTaskScheduler::new();
    assert_eq!(scheduler.call_count(), 0);

    scheduler.try_schedule_ready_tasks().await;
    assert_eq!(scheduler.call_count(), 1);

    scheduler.try_schedule_ready_tasks().await;
    assert_eq!(scheduler.call_count(), 2);
}

#[tokio::test]
async fn test_mock_task_scheduler_clear() {
    let scheduler = MockTaskScheduler::new();
    scheduler.try_schedule_ready_tasks().await;
    assert_eq!(scheduler.call_count(), 1);

    scheduler.clear();
    assert_eq!(scheduler.call_count(), 0);
}

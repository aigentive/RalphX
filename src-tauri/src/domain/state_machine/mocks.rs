// Mock service implementations for testing
// These implementations record calls for verification in tests

use super::services::{
    AgentSpawner, DependencyManager, EventEmitter, NotificationContext, Notifier,
    ReviewStartResult, ReviewStarter, TaskNotification, TaskScheduler,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Records a call to a service method
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCall {
    pub method: String,
    pub args: Vec<String>,
}

impl ServiceCall {
    pub fn new(method: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            method: method.into(),
            args,
        }
    }
}

/// Mock implementation of AgentSpawner that records all calls.
#[derive(Debug, Default)]
pub struct MockAgentSpawner {
    calls: Arc<Mutex<Vec<ServiceCall>>>,
    /// If set, spawn will "fail" by not recording the call
    pub should_fail: Arc<Mutex<bool>>,
}

impl MockAgentSpawner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all recorded calls
    pub fn get_calls(&self) -> Vec<ServiceCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Clears all recorded calls
    pub fn clear(&self) {
        self.calls.lock().unwrap().clear();
    }

    /// Returns the number of spawn calls
    pub fn spawn_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.method == "spawn" || c.method == "spawn_background")
            .count()
    }

    /// Sets whether spawn should "fail"
    pub fn set_should_fail(&self, should_fail: bool) {
        *self.should_fail.lock().unwrap() = should_fail;
    }
}

#[async_trait]
impl AgentSpawner for MockAgentSpawner {
    async fn spawn(&self, agent_type: &str, task_id: &str) {
        if !*self.should_fail.lock().unwrap() {
            self.calls.lock().unwrap().push(ServiceCall::new(
                "spawn",
                vec![agent_type.to_string(), task_id.to_string()],
            ));
        }
    }

    async fn spawn_background(&self, agent_type: &str, task_id: &str) {
        if !*self.should_fail.lock().unwrap() {
            self.calls.lock().unwrap().push(ServiceCall::new(
                "spawn_background",
                vec![agent_type.to_string(), task_id.to_string()],
            ));
        }
    }

    async fn wait_for(&self, agent_type: &str, task_id: &str) {
        self.calls.lock().unwrap().push(ServiceCall::new(
            "wait_for",
            vec![agent_type.to_string(), task_id.to_string()],
        ));
    }

    async fn stop(&self, agent_type: &str, task_id: &str) {
        self.calls.lock().unwrap().push(ServiceCall::new(
            "stop",
            vec![agent_type.to_string(), task_id.to_string()],
        ));
    }
}

/// Mock implementation of EventEmitter that records all emitted events.
#[derive(Debug, Default)]
pub struct MockEventEmitter {
    events: Arc<Mutex<Vec<ServiceCall>>>,
}

impl MockEventEmitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all recorded events
    pub fn get_events(&self) -> Vec<ServiceCall> {
        self.events.lock().unwrap().clone()
    }

    /// Clears all recorded events
    pub fn clear(&self) {
        self.events.lock().unwrap().clear();
    }

    /// Returns the number of events emitted
    pub fn event_count(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Checks if a specific event type was emitted
    pub fn has_event(&self, event_type: &str) -> bool {
        self.events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.args.first().map(|s| s.as_str()) == Some(event_type))
    }
}

#[async_trait]
impl EventEmitter for MockEventEmitter {
    async fn emit(&self, event_type: &str, task_id: &str) {
        self.events.lock().unwrap().push(ServiceCall::new(
            "emit",
            vec![event_type.to_string(), task_id.to_string()],
        ));
    }

    async fn emit_with_payload(&self, event_type: &str, task_id: &str, payload: &str) {
        self.events.lock().unwrap().push(ServiceCall::new(
            "emit_with_payload",
            vec![
                event_type.to_string(),
                task_id.to_string(),
                payload.to_string(),
            ],
        ));
    }

    async fn emit_status_change(&self, task_id: &str, old_status: &str, new_status: &str) {
        self.events.lock().unwrap().push(ServiceCall::new(
            "emit_status_change",
            vec![
                task_id.to_string(),
                old_status.to_string(),
                new_status.to_string(),
            ],
        ));
    }
}

/// Mock implementation of Notifier that records all notifications.
#[derive(Debug, Default)]
pub struct MockNotifier {
    notifications: Arc<Mutex<Vec<ServiceCall>>>,
    notification_records: Arc<Mutex<Vec<NotificationRecord>>>,
}

#[derive(Debug, Clone)]
pub struct NotificationRecord {
    pub context: NotificationContext,
    pub notification: TaskNotification,
}

impl MockNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all recorded notifications
    pub fn get_notifications(&self) -> Vec<ServiceCall> {
        self.notifications.lock().unwrap().clone()
    }

    /// Returns typed notification calls, including their authoritative history context.
    pub fn notification_records(&self) -> Vec<NotificationRecord> {
        self.notification_records.lock().unwrap().clone()
    }

    /// Clears all recorded notifications
    pub fn clear(&self) {
        self.notifications.lock().unwrap().clear();
        self.notification_records.lock().unwrap().clear();
    }

    /// Returns the number of notifications sent
    pub fn notification_count(&self) -> usize {
        self.notifications.lock().unwrap().len()
    }

    /// Checks if a specific notification type was sent
    pub fn has_notification(&self, notification_type: &str) -> bool {
        self.notifications
            .lock()
            .unwrap()
            .iter()
            .any(|n| n.args.first().map(|s| s.as_str()) == Some(notification_type))
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify(&self, context: NotificationContext, notification: TaskNotification) {
        self.notification_records
            .lock()
            .unwrap()
            .push(NotificationRecord {
                context: context.clone(),
                notification: notification.clone(),
            });
        self.notifications.lock().unwrap().push(ServiceCall::new(
            "notify",
            vec![format!("{notification:?}"), context.task.id.to_string()],
        ));
    }
}

/// Mock implementation of DependencyManager that tracks blocker state.
#[derive(Debug, Default)]
pub struct MockDependencyManager {
    calls: Arc<Mutex<Vec<ServiceCall>>>,
    /// Map of task_id -> list of blocking task IDs
    blockers: Arc<Mutex<std::collections::HashMap<String, Vec<String>>>>,
}

impl MockDependencyManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the blockers for a task (for test setup)
    pub fn set_blockers(&self, task_id: impl Into<String>, blocker_ids: Vec<String>) {
        self.blockers
            .lock()
            .unwrap()
            .insert(task_id.into(), blocker_ids);
    }

    /// Removes a blocker from a task
    pub fn remove_blocker(&self, task_id: &str, blocker_id: &str) {
        if let Some(blockers) = self.blockers.lock().unwrap().get_mut(task_id) {
            blockers.retain(|id| id != blocker_id);
        }
    }

    /// Returns all recorded calls
    pub fn get_calls(&self) -> Vec<ServiceCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Clears all recorded calls
    pub fn clear(&self) {
        self.calls.lock().unwrap().clear();
    }
}

#[async_trait]
impl DependencyManager for MockDependencyManager {
    async fn unblock_dependents(&self, completed_task_id: &str) {
        self.calls.lock().unwrap().push(ServiceCall::new(
            "unblock_dependents",
            vec![completed_task_id.to_string()],
        ));

        // Remove this task as a blocker from all tasks
        let mut blockers = self.blockers.lock().unwrap();
        for (_, task_blockers) in blockers.iter_mut() {
            task_blockers.retain(|id| id != completed_task_id);
        }
    }

    async fn has_unresolved_blockers(&self, task_id: &str) -> bool {
        self.calls.lock().unwrap().push(ServiceCall::new(
            "has_unresolved_blockers",
            vec![task_id.to_string()],
        ));

        self.blockers
            .lock()
            .unwrap()
            .get(task_id)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    async fn get_blocking_tasks(&self, task_id: &str) -> Vec<String> {
        self.calls.lock().unwrap().push(ServiceCall::new(
            "get_blocking_tasks",
            vec![task_id.to_string()],
        ));

        self.blockers
            .lock()
            .unwrap()
            .get(task_id)
            .cloned()
            .unwrap_or_default()
    }
}

/// Mock implementation of ReviewStarter that records all calls.
#[derive(Debug)]
pub struct MockReviewStarter {
    calls: Arc<Mutex<Vec<ServiceCall>>>,
    /// The result to return from start_ai_review
    result: Arc<Mutex<ReviewStartResult>>,
    /// Counter for generating review IDs
    counter: Arc<Mutex<u32>>,
}

impl Default for MockReviewStarter {
    fn default() -> Self {
        Self::new()
    }
}

impl MockReviewStarter {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            result: Arc::new(Mutex::new(ReviewStartResult::Started {
                review_id: "review-1".to_string(),
            })),
            counter: Arc::new(Mutex::new(1)),
        }
    }

    /// Creates a mock that returns Disabled
    pub fn disabled() -> Self {
        let mock = Self::new();
        *mock.result.lock().unwrap() = ReviewStartResult::Disabled;
        mock
    }

    /// Creates a mock that returns an error
    pub fn with_error(error: impl Into<String>) -> Self {
        let mock = Self::new();
        *mock.result.lock().unwrap() = ReviewStartResult::Error(error.into());
        mock
    }

    /// Returns all recorded calls
    pub fn get_calls(&self) -> Vec<ServiceCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Clears all recorded calls
    pub fn clear(&self) {
        self.calls.lock().unwrap().clear();
    }

    /// Returns the number of start_ai_review calls
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// Sets the result to return from start_ai_review
    pub fn set_result(&self, result: ReviewStartResult) {
        *self.result.lock().unwrap() = result;
    }

    /// Checks if a review was started for a specific task
    pub fn has_review_for_task(&self, task_id: &str) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.args.first().map(|s| s.as_str()) == Some(task_id))
    }
}

#[async_trait]
impl ReviewStarter for MockReviewStarter {
    async fn start_ai_review(&self, task_id: &str, project_id: &str) -> ReviewStartResult {
        self.calls.lock().unwrap().push(ServiceCall::new(
            "start_ai_review",
            vec![task_id.to_string(), project_id.to_string()],
        ));

        let result = self.result.lock().unwrap().clone();
        match &result {
            ReviewStartResult::Started { .. } => {
                // Generate unique review ID for each call
                let mut counter = self.counter.lock().unwrap();
                let review_id = format!("review-{}", *counter);
                *counter += 1;
                ReviewStartResult::Started { review_id }
            }
            _ => result,
        }
    }
}

/// Mock implementation of TaskScheduler that records all calls.
#[derive(Debug, Default)]
pub struct MockTaskScheduler {
    calls: Arc<Mutex<Vec<ServiceCall>>>,
}

impl MockTaskScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all recorded calls
    pub fn get_calls(&self) -> Vec<ServiceCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Clears all recorded calls
    pub fn clear(&self) {
        self.calls.lock().unwrap().clear();
    }

    /// Returns the number of try_schedule_ready_tasks calls
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait]
impl TaskScheduler for MockTaskScheduler {
    async fn try_schedule_ready_tasks(&self) {
        self.calls
            .lock()
            .unwrap()
            .push(ServiceCall::new("try_schedule_ready_tasks", vec![]));
    }

    async fn try_retry_deferred_merges(&self, project_id: &str) {
        self.calls.lock().unwrap().push(ServiceCall::new(
            "try_retry_deferred_merges",
            vec![project_id.to_string()],
        ));
    }

    async fn try_retry_main_merges(&self) {
        self.calls
            .lock()
            .unwrap()
            .push(ServiceCall::new("try_retry_main_merges", vec![]));
    }
}

#[cfg(test)]
#[path = "mocks_tests.rs"]
mod tests;

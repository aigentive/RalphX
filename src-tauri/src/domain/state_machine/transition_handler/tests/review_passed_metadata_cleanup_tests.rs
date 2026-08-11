// Integration tests for stale freshness metadata cleanup on on_enter(ReviewPassed).
//
// Phase 2, Wave 2C: after a successful review, freshness routing flags
// (freshness_origin_state and freshness_count_incremented_by) must be cleared so
// freshness_routing.rs defense-in-depth is not confused if the task later reaches
// Merging via a different path.
//
// Tests:
//   1. Stale routing flags cleared from task metadata when entering ReviewPassed
//   2. Non-freshness metadata keys preserved after cleanup
//   3. No-op when task_repo is absent (services without repo)

use super::helpers::*;
use crate::domain::entities::{InternalStatus, Project, ProjectId, Task, TaskId};
use crate::domain::repositories::TaskRepository;
use crate::domain::state_machine::context::{TaskContext, TaskServices};
use crate::domain::state_machine::services::{NotificationContext, TaskNotification};
use crate::domain::state_machine::{State, TaskStateMachine, TransitionHandler};
use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};

async fn setup_task_with_freshness_metadata(
    metadata_json: &str,
) -> (
    crate::domain::entities::TaskId,
    Arc<MemoryTaskRepository>,
    Arc<MemoryProjectRepository>,
) {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());

    let mut task = Task::new(project_id.clone(), "Review passed cleanup test".to_string());
    task.internal_status = InternalStatus::ReviewPassed;
    task.metadata = Some(metadata_json.to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new("test-project".to_string(), "/tmp/test-project".to_string());
    project.id = project_id;
    project_repo.create(project).await.unwrap();

    (task_id, task_repo, project_repo)
}

fn build_machine_with_repos(
    task_id: &crate::domain::entities::TaskId,
    task_repo: &Arc<MemoryTaskRepository>,
    project_repo: &Arc<MemoryProjectRepository>,
) -> TaskStateMachine {
    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(
            Arc::clone(project_repo) as Arc<dyn crate::domain::repositories::ProjectRepository>
        );
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    TaskStateMachine::new(context)
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 1: stale routing flags cleared on ReviewPassed entry
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(ReviewPassed) clears freshness_origin_state and freshness_count_incremented_by.
/// branch_freshness_conflict is also cleared (part of RoutingOnly scope).
/// freshness_conflict_count and freshness_auto_reset_count are preserved.
#[tokio::test]
async fn on_enter_review_passed_clears_freshness_routing_flags() {
    let stale_metadata = serde_json::json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "reviewing",
        "freshness_count_incremented_by": "ensure_branches_fresh",
        "freshness_conflict_count": 2,
        "freshness_auto_reset_count": 1,
        "plan_update_conflict": true,
        "source_update_conflict": false,
        "trigger_origin": "scheduler"
    })
    .to_string();

    let (task_id, task_repo, project_repo) =
        setup_task_with_freshness_metadata(&stale_metadata).await;
    let mut machine = build_machine_with_repos(&task_id, &task_repo, &project_repo);

    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::ReviewPassed).await;
    assert!(
        result.is_ok(),
        "on_enter(ReviewPassed) should succeed: {result:?}"
    );

    // Read back task metadata from repo
    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    let meta: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap_or("{}")).unwrap();

    // Routing flags cleared
    assert_eq!(
        meta.get("branch_freshness_conflict")
            .and_then(|v| v.as_bool()),
        Some(false),
        "branch_freshness_conflict should be false"
    );
    assert!(
        meta.get("freshness_origin_state").is_none() || meta["freshness_origin_state"].is_null(),
        "freshness_origin_state should be absent"
    );
    assert!(
        meta.get("freshness_count_incremented_by").is_none()
            || meta["freshness_count_incremented_by"].is_null(),
        "freshness_count_incremented_by should be absent"
    );
    assert_eq!(
        meta.get("plan_update_conflict").and_then(|v| v.as_bool()),
        Some(false),
        "plan_update_conflict should be false"
    );

    // Conflict count preserved (RoutingOnly scope preserves counts)
    assert_eq!(
        meta.get("freshness_conflict_count")
            .and_then(|v| v.as_i64()),
        Some(2),
        "freshness_conflict_count should be preserved"
    );
    assert_eq!(
        meta.get("freshness_auto_reset_count")
            .and_then(|v| v.as_i64()),
        Some(1),
        "freshness_auto_reset_count should be preserved"
    );

    // Non-freshness keys preserved
    assert_eq!(
        meta.get("trigger_origin").and_then(|v| v.as_str()),
        Some("scheduler"),
        "trigger_origin should be preserved"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2: no-op when task has no stale routing flags
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(ReviewPassed) is idempotent: if routing flags are already clear,
/// metadata is updated but values remain the same.
#[tokio::test]
async fn on_enter_review_passed_noop_when_no_stale_flags() {
    let clean_metadata = serde_json::json!({
        "trigger_origin": "scheduler",
        "freshness_conflict_count": 0
    })
    .to_string();

    let (task_id, task_repo, project_repo) =
        setup_task_with_freshness_metadata(&clean_metadata).await;
    let mut machine = build_machine_with_repos(&task_id, &task_repo, &project_repo);

    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::ReviewPassed).await;
    assert!(result.is_ok());

    let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let meta: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap_or("{}")).unwrap();

    // Non-freshness key preserved
    assert_eq!(
        meta.get("trigger_origin").and_then(|v| v.as_str()),
        Some("scheduler")
    );
    // No routing flags appear
    assert!(
        meta.get("freshness_origin_state").is_none() || meta["freshness_origin_state"].is_null()
    );
    assert!(
        meta.get("freshness_count_incremented_by").is_none()
            || meta["freshness_count_incremented_by"].is_null()
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3: no-op without task_repo (services without repo configured)
// ──────────────────────────────────────────────────────────────────────────────

/// on_enter(ReviewPassed) gracefully skips metadata cleanup when task_repo is absent.
/// Still emits events and notifies.
#[tokio::test]
async fn on_enter_review_passed_skips_cleanup_without_task_repo() {
    let (_spawner, emitter, notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "Review passed notification task".to_string(),
    );
    task.id = TaskId::from_string("task-no-repo".to_string());
    let context = create_context_with_services(
        "task-no-repo",
        "proj-1",
        services.with_notification_context(NotificationContext {
            task,
            history_entry_id: "committed-history-review-passed".to_string(),
            project_id,
        }),
    );
    let mut machine = TaskStateMachine::new(context);

    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::ReviewPassed).await;

    // Should succeed even without a task_repo
    assert!(
        result.is_ok(),
        "on_enter without task_repo should not error"
    );

    // Events and typed notifications still fired without a task repository.
    assert!(emitter.has_event("review:ai_approved"));
    let notifications = notifier.notification_records();
    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        notifications[0].notification,
        TaskNotification::StateEntered(InternalStatus::ReviewPassed)
    ));
    assert_eq!(notifications[0].context.task.id.as_str(), "task-no-repo");
    assert_eq!(
        notifications[0].context.history_entry_id,
        "committed-history-review-passed"
    );
}

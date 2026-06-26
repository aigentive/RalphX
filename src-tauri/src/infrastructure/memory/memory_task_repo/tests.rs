use crate::domain::entities::{InternalStatus, ProjectId, Task, TaskId};
use crate::domain::repositories::TaskRepository;
use crate::infrastructure::memory::MemoryTaskRepository;

fn create_test_task(project_id: ProjectId, title: &str, priority: i32) -> Task {
    let mut task = Task::new(project_id, title.to_string());
    task.priority = priority;
    task
}

// ===== CRUD Tests =====

#[tokio::test]
async fn test_create_returns_task_with_id() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Test task".to_string());

    let result = repo.create(task.clone()).await.unwrap();

    assert_eq!(result.id, task.id);
    assert_eq!(result.title, "Test task");
}

#[tokio::test]
async fn test_get_by_id_returns_task() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Find me".to_string());
    repo.create(task.clone()).await.unwrap();

    let result = repo.get_by_id(&task.id).await.unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap().title, "Find me");
}

#[tokio::test]
async fn test_get_by_id_returns_none_for_missing() {
    let repo = MemoryTaskRepository::new();
    let task_id = TaskId::new();

    let result = repo.get_by_id(&task_id).await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_by_project_filters_correctly() {
    let repo = MemoryTaskRepository::new();
    let project1 = ProjectId::new();
    let project2 = ProjectId::new();

    repo.create(create_test_task(project1.clone(), "P1 Task 1", 1))
        .await
        .unwrap();
    repo.create(create_test_task(project1.clone(), "P1 Task 2", 2))
        .await
        .unwrap();
    repo.create(create_test_task(project2.clone(), "P2 Task 1", 3))
        .await
        .unwrap();

    let p1_tasks = repo.get_by_project(&project1).await.unwrap();
    let p2_tasks = repo.get_by_project(&project2).await.unwrap();

    assert_eq!(p1_tasks.len(), 2);
    assert_eq!(p2_tasks.len(), 1);
    assert!(p1_tasks.iter().all(|t| t.project_id == project1));
}

#[tokio::test]
async fn test_get_by_project_sorts_by_priority_and_created_at() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    // Create tasks with different priorities
    repo.create(create_test_task(project.clone(), "Low", 1))
        .await
        .unwrap();
    repo.create(create_test_task(project.clone(), "High", 10))
        .await
        .unwrap();
    repo.create(create_test_task(project.clone(), "Medium", 5))
        .await
        .unwrap();

    let tasks = repo.get_by_project(&project).await.unwrap();

    assert_eq!(tasks[0].title, "High");
    assert_eq!(tasks[1].title, "Medium");
    assert_eq!(tasks[2].title, "Low");
}

#[tokio::test]
async fn test_update_modifies_existing_task() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let mut task = Task::new(project_id, "Original".to_string());
    repo.create(task.clone()).await.unwrap();

    task.title = "Updated".to_string();
    task.priority = 100;
    repo.update(&task).await.unwrap();

    let result = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(result.title, "Updated");
    assert_eq!(result.priority, 100);
}

#[tokio::test]
async fn test_delete_removes_task() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Delete me".to_string());
    repo.create(task.clone()).await.unwrap();

    repo.delete(&task.id).await.unwrap();

    let result = repo.get_by_id(&task.id).await.unwrap();
    assert!(result.is_none());
}

// ===== Status Operations Tests =====

#[tokio::test]
async fn test_get_by_status_filters_correctly() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let mut task1 = create_test_task(project.clone(), "Backlog", 1);
    task1.internal_status = InternalStatus::Backlog;

    let mut task2 = create_test_task(project.clone(), "Ready", 2);
    task2.internal_status = InternalStatus::Ready;

    let mut task3 = create_test_task(project.clone(), "Also Ready", 3);
    task3.internal_status = InternalStatus::Ready;

    repo.create(task1).await.unwrap();
    repo.create(task2).await.unwrap();
    repo.create(task3).await.unwrap();

    let ready = repo
        .get_by_status(&project, InternalStatus::Ready)
        .await
        .unwrap();
    let backlog = repo
        .get_by_status(&project, InternalStatus::Backlog)
        .await
        .unwrap();

    assert_eq!(ready.len(), 2);
    assert_eq!(backlog.len(), 1);
}

#[tokio::test]
async fn test_persist_status_change_updates_task() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Status test".to_string());
    repo.create(task.clone()).await.unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Backlog,
        InternalStatus::Ready,
        "user",
    )
    .await
    .unwrap();

    let updated = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_persist_status_change_records_history() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "History test".to_string());
    repo.create(task.clone()).await.unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Backlog,
        InternalStatus::Ready,
        "user",
    )
    .await
    .unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Ready,
        InternalStatus::Executing,
        "agent",
    )
    .await
    .unwrap();

    let history = repo.get_status_history(&task.id).await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].from, InternalStatus::Backlog);
    assert_eq!(history[0].to, InternalStatus::Ready);
    assert_eq!(history[0].trigger, "user");
    assert_eq!(history[1].from, InternalStatus::Ready);
    assert_eq!(history[1].to, InternalStatus::Executing);
    assert_eq!(history[1].trigger, "agent");
}

#[tokio::test]
async fn test_get_status_history_empty_for_new_task() {
    let repo = MemoryTaskRepository::new();
    let task_id = TaskId::new();

    let history = repo.get_status_history(&task_id).await.unwrap();
    assert!(history.is_empty());
}

#[tokio::test]
async fn test_get_status_entered_at_returns_earliest_timestamp() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Status timestamp test".to_string());
    repo.create(task.clone()).await.unwrap();

    // Transition through multiple statuses
    repo.persist_status_change(
        &task.id,
        InternalStatus::Backlog,
        InternalStatus::Ready,
        "user",
    )
    .await
    .unwrap();

    // Small delay to ensure different timestamps
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    repo.persist_status_change(
        &task.id,
        InternalStatus::Ready,
        InternalStatus::Executing,
        "agent",
    )
    .await
    .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    repo.persist_status_change(
        &task.id,
        InternalStatus::Executing,
        InternalStatus::PendingMerge,
        "agent",
    )
    .await
    .unwrap();

    // Get timestamp for when task first entered PendingMerge
    let timestamp = repo
        .get_status_entered_at(&task.id, InternalStatus::PendingMerge)
        .await
        .unwrap();

    assert!(timestamp.is_some());

    // Verify it's the only transition to PendingMerge
    let history = repo.get_status_history(&task.id).await.unwrap();
    let pending_merge_transitions: Vec<_> = history
        .iter()
        .filter(|t| t.to == InternalStatus::PendingMerge)
        .collect();
    assert_eq!(pending_merge_transitions.len(), 1);
    assert_eq!(timestamp.unwrap(), pending_merge_transitions[0].timestamp);
}

#[tokio::test]
async fn test_get_status_entered_at_returns_none_if_never_entered() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Never merged".to_string());
    repo.create(task.clone()).await.unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Backlog,
        InternalStatus::Ready,
        "user",
    )
    .await
    .unwrap();

    // Task never entered Merging status
    let timestamp = repo
        .get_status_entered_at(&task.id, InternalStatus::Merging)
        .await
        .unwrap();

    assert!(timestamp.is_none());
}

#[tokio::test]
async fn test_get_status_entered_at_returns_earliest_when_entered_multiple_times() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Re-enter status".to_string());
    repo.create(task.clone()).await.unwrap();

    // Enter PendingMerge first time
    repo.persist_status_change(
        &task.id,
        InternalStatus::Executing,
        InternalStatus::PendingMerge,
        "agent",
    )
    .await
    .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Transition away
    repo.persist_status_change(
        &task.id,
        InternalStatus::PendingMerge,
        InternalStatus::MergeIncomplete,
        "system",
    )
    .await
    .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Re-enter PendingMerge (e.g., after fixing merge issue)
    repo.persist_status_change(
        &task.id,
        InternalStatus::Executing,
        InternalStatus::PendingMerge,
        "agent",
    )
    .await
    .unwrap();

    // Should return the FIRST time it entered PendingMerge
    let timestamp = repo
        .get_status_entered_at(&task.id, InternalStatus::PendingMerge)
        .await
        .unwrap()
        .unwrap();

    let history = repo.get_status_history(&task.id).await.unwrap();
    let first_pending_merge = history
        .iter()
        .find(|t| t.to == InternalStatus::PendingMerge)
        .unwrap();

    assert_eq!(timestamp, first_pending_merge.timestamp);
}

#[tokio::test]
async fn test_get_status_last_entered_at_returns_latest_when_entered_multiple_times() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Re-enter status".to_string());
    repo.create(task.clone()).await.unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Executing,
        InternalStatus::PendingMerge,
        "agent",
    )
    .await
    .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    repo.persist_status_change(
        &task.id,
        InternalStatus::PendingMerge,
        InternalStatus::MergeIncomplete,
        "system",
    )
    .await
    .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    repo.persist_status_change(
        &task.id,
        InternalStatus::Executing,
        InternalStatus::PendingMerge,
        "agent",
    )
    .await
    .unwrap();

    let timestamp = repo
        .get_status_last_entered_at(&task.id, InternalStatus::PendingMerge)
        .await
        .unwrap()
        .unwrap();

    let history = repo.get_status_history(&task.id).await.unwrap();
    let latest_pending_merge = history
        .iter()
        .rev()
        .find(|t| t.to == InternalStatus::PendingMerge)
        .unwrap();

    assert_eq!(timestamp, latest_pending_merge.timestamp);
}

#[tokio::test]
async fn test_get_status_entered_at_returns_none_for_nonexistent_task() {
    let repo = MemoryTaskRepository::new();
    let task_id = TaskId::new();

    let timestamp = repo
        .get_status_entered_at(&task_id, InternalStatus::Ready)
        .await
        .unwrap();

    assert!(timestamp.is_none());
}

#[tokio::test]
async fn test_get_status_entered_at_works_for_various_statuses() {
    let repo = MemoryTaskRepository::new();
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Multi-status".to_string());
    repo.create(task.clone()).await.unwrap();

    // Transition through several statuses
    let statuses = vec![
        (InternalStatus::Backlog, InternalStatus::Ready),
        (InternalStatus::Ready, InternalStatus::Executing),
        (InternalStatus::Executing, InternalStatus::PendingReview),
        (InternalStatus::PendingReview, InternalStatus::Approved),
    ];

    for (from, to) in statuses {
        repo.persist_status_change(&task.id, from, to, "test")
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    }

    // Verify each status has a timestamp
    for status in [
        InternalStatus::Ready,
        InternalStatus::Executing,
        InternalStatus::PendingReview,
        InternalStatus::Approved,
    ] {
        let timestamp = repo.get_status_entered_at(&task.id, status).await.unwrap();
        assert!(
            timestamp.is_some(),
            "Expected timestamp for status {:?}",
            status
        );
    }

    // Backlog was the starting status, never transitioned TO it
    let backlog_timestamp = repo
        .get_status_entered_at(&task.id, InternalStatus::Backlog)
        .await
        .unwrap();
    assert!(backlog_timestamp.is_none());
}

// ===== Query Operations Tests =====

#[tokio::test]
async fn test_get_next_executable_returns_ready_task() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let mut task = create_test_task(project.clone(), "Ready", 10);
    task.internal_status = InternalStatus::Ready;
    repo.create(task.clone()).await.unwrap();

    let mut backlog = create_test_task(project.clone(), "Backlog", 100);
    backlog.internal_status = InternalStatus::Backlog;
    repo.create(backlog).await.unwrap();

    let next = repo.get_next_executable(&project).await.unwrap();
    assert!(next.is_some());
    assert_eq!(next.unwrap().title, "Ready");
}

#[tokio::test]
async fn test_get_next_executable_respects_priority() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let mut low = create_test_task(project.clone(), "Low", 1);
    low.internal_status = InternalStatus::Ready;
    repo.create(low).await.unwrap();

    let mut high = create_test_task(project.clone(), "High", 100);
    high.internal_status = InternalStatus::Ready;
    repo.create(high).await.unwrap();

    let next = repo.get_next_executable(&project).await.unwrap();
    assert_eq!(next.unwrap().title, "High");
}

#[tokio::test]
async fn test_get_next_executable_excludes_blocked() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let mut task = create_test_task(project.clone(), "Blocked", 100);
    task.internal_status = InternalStatus::Ready;
    repo.create(task.clone()).await.unwrap();

    // Note: dependency checking is now done via TaskDependencyRepository.
    // The in-memory TaskRepository.get_next_executable() no longer filters by blockers.
    let next = repo.get_next_executable(&project).await.unwrap();
    assert!(next.is_some()); // Returns the ready task (blocker filtering is in TaskDependencyRepo)
}

#[tokio::test]
async fn test_get_next_executable_returns_none_when_empty() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let next = repo.get_next_executable(&project).await.unwrap();
    assert!(next.is_none());
}

// Note: blocker operation tests removed — blockers are now managed via TaskDependencyRepository.
// See memory_task_dependency_repo_tests.rs for dependency tests.

// ===== with_tasks Constructor Test =====

#[tokio::test]
async fn test_with_tasks_prepopulates() {
    let project = ProjectId::new();
    let task1 = create_test_task(project.clone(), "Prepop 1", 1);
    let task2 = create_test_task(project.clone(), "Prepop 2", 2);

    let repo = MemoryTaskRepository::with_tasks(vec![task1.clone(), task2.clone()]);

    let result = repo.get_by_project(&project).await.unwrap();
    assert_eq!(result.len(), 2);
}

// ===== Archive Operations Tests =====

#[tokio::test]
async fn test_archive_sets_archived_at() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();
    let task = create_test_task(project, "Task to Archive", 1);
    repo.create(task.clone()).await.unwrap();

    let archived = repo.archive(&task.id).await.unwrap();
    assert!(archived.archived_at.is_some());

    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(found.archived_at.is_some());
}

#[tokio::test]
async fn test_restore_clears_archived_at() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();
    let task = create_test_task(project, "Task to Restore", 1);
    repo.create(task.clone()).await.unwrap();

    repo.archive(&task.id).await.unwrap();
    let restored = repo.restore(&task.id).await.unwrap();
    assert!(restored.archived_at.is_none());

    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(found.archived_at.is_none());
}

#[tokio::test]
async fn test_get_archived_count_returns_correct_count() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let task1 = create_test_task(project.clone(), "Task 1", 1);
    let task2 = create_test_task(project.clone(), "Task 2", 2);
    let task3 = create_test_task(project.clone(), "Task 3", 3);

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.create(task3.clone()).await.unwrap();

    repo.archive(&task1.id).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    let count = repo.get_archived_count(&project, None).await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_get_by_project_filtered_excludes_archived_by_default() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let task1 = create_test_task(project.clone(), "Active", 1);
    let task2 = create_test_task(project.clone(), "Archived", 2);

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    let active = repo.get_by_project_filtered(&project, false).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].title, "Active");
}

#[tokio::test]
async fn test_get_by_project_filtered_includes_archived_when_requested() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let task1 = create_test_task(project.clone(), "Active", 1);
    let task2 = create_test_task(project.clone(), "Archived", 2);

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    let all = repo.get_by_project_filtered(&project, true).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn test_archive_nonexistent_task_returns_error() {
    let repo = MemoryTaskRepository::new();
    let task_id = TaskId::new();

    let result = repo.archive(&task_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_restore_nonexistent_task_returns_error() {
    let repo = MemoryTaskRepository::new();
    let task_id = TaskId::new();

    let result = repo.restore(&task_id).await;
    assert!(result.is_err());
}

// ===== Search Operations Tests =====

#[tokio::test]
async fn test_search_by_title() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let task1 = create_test_task(project.clone(), "Implement authentication", 1);
    let task2 = create_test_task(project.clone(), "Add user login", 2);
    let task3 = create_test_task(project.clone(), "Fix database bug", 3);

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.create(task3.clone()).await.unwrap();

    // Search for "auth" - should match "authentication"
    let results = repo.search(&project, "auth", false).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, task1.id);
}

#[tokio::test]
async fn test_search_by_description() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let mut task1 = create_test_task(project.clone(), "Task One", 1);
    task1.description = Some("This task implements authentication".to_string());

    let mut task2 = create_test_task(project.clone(), "Task Two", 2);
    task2.description = Some("This task adds logging".to_string());

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();

    // Search for "authentication" - should match description
    let results = repo
        .search(&project, "authentication", false)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, task1.id);
}

#[tokio::test]
async fn test_search_case_insensitive() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let task = create_test_task(project.clone(), "Add USER Authentication", 1);
    repo.create(task.clone()).await.unwrap();

    // Search with lowercase - should match
    let results = repo.search(&project, "user", false).await.unwrap();
    assert_eq!(results.len(), 1);

    // Search with uppercase - should also match
    let results = repo.search(&project, "USER", false).await.unwrap();
    assert_eq!(results.len(), 1);

    // Search with mixed case - should also match
    let results = repo.search(&project, "UsEr", false).await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_search_returns_empty_for_no_match() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let task = create_test_task(project.clone(), "Add user login", 1);
    repo.create(task.clone()).await.unwrap();

    // Search for something that doesn't exist
    let results = repo.search(&project, "nonexistent", false).await.unwrap();
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_search_excludes_archived_by_default() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let task1 = create_test_task(project.clone(), "Active authentication task", 1);
    let task2 = create_test_task(project.clone(), "Archived authentication task", 2);

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    // Search without including archived - should only find active task
    let results = repo
        .search(&project, "authentication", false)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, task1.id);
}

#[tokio::test]
async fn test_search_includes_archived_when_requested() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let task1 = create_test_task(project.clone(), "Active authentication task", 1);
    let task2 = create_test_task(project.clone(), "Archived authentication task", 2);

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    // Search with including archived - should find both tasks
    let results = repo.search(&project, "authentication", true).await.unwrap();
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_search_matches_partial_strings() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let task = create_test_task(project.clone(), "Implement user authentication system", 1);
    repo.create(task.clone()).await.unwrap();

    // Search for partial match
    let results = repo.search(&project, "authen", false).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, task.id);
}

#[tokio::test]
async fn test_search_matches_in_title_or_description() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let mut task1 = create_test_task(project.clone(), "Add logging feature", 1);
    task1.description = Some("Implement authentication logging".to_string());

    let task2 = create_test_task(project.clone(), "Authentication system", 2);

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();

    // Search for "authentication" - should match both (title and description)
    let results = repo
        .search(&project, "authentication", false)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
}

// ===== Cross-Project Ready Task Tests (Phase 26 - Auto-Scheduler) =====

#[tokio::test]
async fn test_get_oldest_ready_task_returns_oldest_by_created_at() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    // Create older Ready task first
    let mut older_task = create_test_task(project.clone(), "Older Task", 1);
    older_task.internal_status = InternalStatus::Ready;
    repo.create(older_task.clone()).await.unwrap();

    // Small delay to ensure different created_at
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Create newer Ready task
    let mut newer_task = create_test_task(project.clone(), "Newer Task", 2);
    newer_task.internal_status = InternalStatus::Ready;
    repo.create(newer_task.clone()).await.unwrap();

    let result = repo.get_oldest_ready_task().await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().id, older_task.id);
}

#[tokio::test]
async fn test_get_oldest_ready_task_across_projects() {
    let repo = MemoryTaskRepository::new();
    let project1 = ProjectId::new();
    let project2 = ProjectId::new();

    // Create older Ready task in project 2
    let mut older_task = Task::new(project2, "Older Task (P2)".to_string());
    older_task.internal_status = InternalStatus::Ready;
    repo.create(older_task.clone()).await.unwrap();

    // Small delay
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Create newer Ready task in project 1
    let mut newer_task = Task::new(project1, "Newer Task (P1)".to_string());
    newer_task.internal_status = InternalStatus::Ready;
    repo.create(newer_task.clone()).await.unwrap();

    let result = repo.get_oldest_ready_task().await.unwrap();
    assert!(result.is_some());
    assert_eq!(
        result.unwrap().id,
        older_task.id,
        "Should return oldest task regardless of project"
    );
}

#[tokio::test]
async fn test_get_oldest_ready_task_excludes_non_ready() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    // Create Backlog task (older)
    let mut backlog_task = create_test_task(project.clone(), "Backlog Task", 1);
    backlog_task.internal_status = InternalStatus::Backlog;
    repo.create(backlog_task.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Create Ready task (newer)
    let mut ready_task = create_test_task(project.clone(), "Ready Task", 2);
    ready_task.internal_status = InternalStatus::Ready;
    repo.create(ready_task.clone()).await.unwrap();

    let result = repo.get_oldest_ready_task().await.unwrap();
    assert!(result.is_some());
    assert_eq!(
        result.unwrap().id,
        ready_task.id,
        "Should only return Ready tasks"
    );
}

#[tokio::test]
async fn test_get_oldest_ready_task_excludes_archived() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    // Create older Ready task and archive it
    let mut archived_task = create_test_task(project.clone(), "Archived Ready", 1);
    archived_task.internal_status = InternalStatus::Ready;
    repo.create(archived_task.clone()).await.unwrap();
    repo.archive(&archived_task.id).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Create active Ready task (newer)
    let mut active_task = create_test_task(project.clone(), "Active Ready", 2);
    active_task.internal_status = InternalStatus::Ready;
    repo.create(active_task.clone()).await.unwrap();

    let result = repo.get_oldest_ready_task().await.unwrap();
    assert!(result.is_some());
    assert_eq!(
        result.unwrap().id,
        active_task.id,
        "Should exclude archived tasks"
    );
}

#[tokio::test]
async fn test_get_oldest_ready_task_returns_none_when_no_ready_tasks() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    // Create tasks in non-Ready statuses
    let mut backlog = create_test_task(project.clone(), "Backlog", 1);
    backlog.internal_status = InternalStatus::Backlog;
    repo.create(backlog).await.unwrap();

    let mut executing = create_test_task(project.clone(), "Executing", 2);
    executing.internal_status = InternalStatus::Executing;
    repo.create(executing).await.unwrap();

    let result = repo.get_oldest_ready_task().await.unwrap();
    assert!(
        result.is_none(),
        "Should return None when no Ready tasks exist"
    );
}

#[tokio::test]
async fn test_get_oldest_ready_task_returns_none_when_empty() {
    let repo = MemoryTaskRepository::new();
    let result = repo.get_oldest_ready_task().await.unwrap();
    assert!(result.is_none(), "Should return None for empty repository");
}

#[tokio::test]
async fn test_get_by_status_excludes_archived() {
    let repo = MemoryTaskRepository::new();
    let project = ProjectId::new();

    let mut task1 = create_test_task(project.clone(), "Active PendingMerge", 1);
    task1.internal_status = InternalStatus::PendingMerge;

    let mut task2 = create_test_task(project.clone(), "Archived PendingMerge", 2);
    task2.internal_status = InternalStatus::PendingMerge;

    repo.create(task1).await.unwrap();
    repo.create(task2.clone()).await.unwrap();

    // Archive the second task
    repo.archive(&task2.id).await.unwrap();

    let results = repo
        .get_by_status(&project, InternalStatus::PendingMerge)
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Active PendingMerge");
}

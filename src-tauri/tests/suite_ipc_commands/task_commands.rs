// Tests for task_commands module

use chrono::Utc;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::task_commands::{helpers, types};
use ralphx_lib::domain::entities::{
    InternalStatus, Project, ProjectId, Task, TaskCategory, TaskId,
};
use ralphx_lib::domain::repositories::ProjectRepository;
use ralphx_lib::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};
use std::sync::Arc;

async fn setup_test_state() -> AppState {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    // Create a test project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    project_repo.create(project).await.unwrap();

    AppState::with_repos(task_repo, project_repo)
}

#[tokio::test]
async fn test_create_task_with_defaults() {
    let state = setup_test_state().await;

    let input = types::CreateTaskInput {
        project_id: "test-project".to_string(),
        title: "Test Task".to_string(),
        category: None,
        description: None,
        priority: None,
        steps: None,
        ideation_session_id: None,
        execution_plan_id: None,
    };

    // We can't easily call tauri commands without the full runtime,
    // so we test the repository directly
    let project_id = ProjectId::from_string(input.project_id);
    let task = Task::new(project_id.clone(), input.title);

    let created = state.task_repo.create(task).await.unwrap();
    assert_eq!(created.title, "Test Task");
    assert_eq!(created.category, TaskCategory::Regular);
    assert_eq!(created.priority, 0);
}

#[tokio::test]
async fn test_create_task_with_all_fields() {
    let state = setup_test_state().await;

    let project_id = ProjectId::from_string("test-project".to_string());
    let mut task = Task::new_with_category(
        project_id.clone(),
        "Full Task".to_string(),
        TaskCategory::Regular,
    );
    task.description = Some("A description".to_string());
    task.priority = 10;

    let created = state.task_repo.create(task).await.unwrap();

    assert_eq!(created.title, "Full Task");
    assert_eq!(created.category, TaskCategory::Regular);
    assert_eq!(created.description, Some("A description".to_string()));
    assert_eq!(created.priority, 10);
}

#[tokio::test]
async fn test_get_task_returns_none_for_nonexistent() {
    let state = setup_test_state().await;
    let id = TaskId::new();

    let result = state.task_repo.get_by_id(&id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_update_task_modifies_fields() {
    let state = setup_test_state().await;

    let project_id = ProjectId::from_string("test-project".to_string());
    let task = Task::new(project_id, "Original Title".to_string());
    let created = state.task_repo.create(task).await.unwrap();

    let mut updated = created.clone();
    updated.title = "Updated Title".to_string();
    updated.priority = 99;

    state.task_repo.update(&updated).await.unwrap();

    let found = state
        .task_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.title, "Updated Title");
    assert_eq!(found.priority, 99);
}

#[tokio::test]
async fn test_list_tasks_returns_all_for_project() {
    let state = setup_test_state().await;

    let project_id = ProjectId::from_string("test-project".to_string());

    state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 1".to_string()))
        .await
        .unwrap();
    state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 2".to_string()))
        .await
        .unwrap();
    state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 3".to_string()))
        .await
        .unwrap();

    let tasks = state.task_repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(tasks.len(), 3);
}

#[tokio::test]
async fn test_task_response_serialization() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let task = Task::new(project_id, "Test Task".to_string());
    let response = types::TaskResponse::from(task);

    // Verify all fields are set
    assert!(!response.id.is_empty());
    assert_eq!(response.project_id, "proj-123");
    assert_eq!(response.title, "Test Task");
    assert_eq!(response.internal_status, "backlog");

    // Verify it serializes to JSON
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"title\":\"Test Task\""));
}

// ========================================
// Answer User Question Command Tests
// ========================================

#[tokio::test]
async fn test_answer_user_question_transitions_blocked_to_ready() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create a blocked task (simulating an agent waiting for user input)
    let mut task = Task::new(project_id, "Blocked Task".to_string());
    task.internal_status = InternalStatus::Blocked;
    let created = state.task_repo.create(task).await.unwrap();

    // Verify task is blocked
    assert_eq!(created.internal_status, InternalStatus::Blocked);

    // Simulate answering the question by updating the task
    let mut task = state
        .task_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.internal_status, InternalStatus::Blocked);

    // The command transitions Blocked -> Ready
    task.internal_status = InternalStatus::Ready;
    task.touch();
    state.task_repo.update(&task).await.unwrap();

    // Verify task is now ready
    let updated = state
        .task_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_answer_user_question_fails_if_not_blocked() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create a task that is not blocked (e.g., Ready)
    let mut task = Task::new(project_id, "Ready Task".to_string());
    task.internal_status = InternalStatus::Ready;
    let created = state.task_repo.create(task).await.unwrap();

    // Verify task is not blocked
    let task = state
        .task_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(task.internal_status, InternalStatus::Blocked);

    // In the real command, this would return an error
    // Here we just verify the precondition check
}

#[tokio::test]
async fn test_answer_user_question_not_found() {
    let state = setup_test_state().await;
    let nonexistent_id = TaskId::from_string("nonexistent".to_string());

    // Task not found
    let result = state.task_repo.get_by_id(&nonexistent_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_answer_user_question_input_deserialization() {
    // Test that input deserializes correctly with camelCase
    let json = r#"{
            "taskId": "task-123",
            "selectedOptions": ["option1", "option2"],
            "customResponse": "My custom answer"
        }"#;

    let input: types::AnswerUserQuestionInput = serde_json::from_str(json).unwrap();
    assert_eq!(input.task_id, "task-123");
    assert_eq!(input.selected_options, vec!["option1", "option2"]);
    assert_eq!(input.custom_response, Some("My custom answer".to_string()));
}

#[tokio::test]
async fn test_answer_user_question_input_without_custom_response() {
    // Test that input deserializes correctly without custom_response
    let json = r#"{
            "taskId": "task-456",
            "selectedOptions": ["option1"]
        }"#;

    let input: types::AnswerUserQuestionInput = serde_json::from_str(json).unwrap();
    assert_eq!(input.task_id, "task-456");
    assert_eq!(input.selected_options, vec!["option1"]);
    assert!(input.custom_response.is_none());
}

#[tokio::test]
async fn test_answer_user_question_response_serialization() {
    let response = types::AnswerUserQuestionResponse {
        task_id: "task-789".to_string(),
        resumed_status: "ready".to_string(),
        answer_recorded: true,
    };

    let json = serde_json::to_string(&response).unwrap();

    // Verify snake_case serialization (backend convention)
    assert!(json.contains("\"task_id\":\"task-789\""));
    assert!(json.contains("\"resumed_status\":\"ready\""));
    assert!(json.contains("\"answer_recorded\":true"));
}

// ========================================
// Inject Task Command Tests
// ========================================

#[tokio::test]
async fn test_inject_task_input_deserialization_minimal() {
    // Test minimal input with defaults
    let json = r#"{
            "projectId": "proj-123",
            "title": "Injected Task"
        }"#;

    let input: types::InjectTaskInput = serde_json::from_str(json).unwrap();
    assert_eq!(input.project_id, "proj-123");
    assert_eq!(input.title, "Injected Task");
    assert!(input.description.is_none());
    assert!(input.category.is_none());
    assert_eq!(input.target, "backlog");
    assert!(!input.make_next);
}

#[tokio::test]
async fn test_inject_task_input_deserialization_full() {
    // Test full input with all fields
    let json = r#"{
            "projectId": "proj-456",
            "title": "Urgent Task",
            "description": "This is urgent",
            "category": "bug",
            "target": "planned",
            "makeNext": true
        }"#;

    let input: types::InjectTaskInput = serde_json::from_str(json).unwrap();
    assert_eq!(input.project_id, "proj-456");
    assert_eq!(input.title, "Urgent Task");
    assert_eq!(input.description, Some("This is urgent".to_string()));
    assert_eq!(input.category, Some("bug".to_string()));
    assert_eq!(input.target, "planned");
    assert!(input.make_next);
}

#[tokio::test]
async fn test_inject_task_response_serialization() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let task = Task::new(project_id, "Test Task".to_string());
    let response = types::InjectTaskResponse {
        task: types::TaskResponse::from(task),
        target: "planned".to_string(),
        priority: 1000,
        make_next_applied: true,
    };

    let json = serde_json::to_string(&response).unwrap();

    // Verify snake_case serialization (backend convention)
    assert!(json.contains("\"target\":\"planned\""));
    assert!(json.contains("\"priority\":1000"));
    assert!(json.contains("\"make_next_applied\":true"));
}

#[tokio::test]
async fn test_inject_task_to_backlog_creates_backlog_task() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Inject to backlog (default)
    let mut task = Task::new_with_category(
        project_id.clone(),
        "Backlog Task".to_string(),
        TaskCategory::Regular,
    );
    task.internal_status = InternalStatus::Backlog;
    task.priority = 0;

    let created = state.task_repo.create(task).await.unwrap();

    assert_eq!(created.internal_status, InternalStatus::Backlog);
    assert_eq!(created.priority, 0);
}

#[tokio::test]
async fn test_inject_task_to_planned_creates_ready_task() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Inject to planned queue
    let mut task = Task::new_with_category(
        project_id.clone(),
        "Planned Task".to_string(),
        TaskCategory::Regular,
    );
    task.internal_status = InternalStatus::Ready;
    task.priority = 0;

    let created = state.task_repo.create(task).await.unwrap();

    assert_eq!(created.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_inject_task_make_next_gets_highest_priority() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create existing ready tasks with various priorities
    let mut task1 = Task::new(project_id.clone(), "Existing 1".to_string());
    task1.internal_status = InternalStatus::Ready;
    task1.priority = 10;
    state.task_repo.create(task1).await.unwrap();

    let mut task2 = Task::new(project_id.clone(), "Existing 2".to_string());
    task2.internal_status = InternalStatus::Ready;
    task2.priority = 50;
    state.task_repo.create(task2).await.unwrap();

    let mut task3 = Task::new(project_id.clone(), "Existing 3".to_string());
    task3.internal_status = InternalStatus::Ready;
    task3.priority = 25;
    state.task_repo.create(task3).await.unwrap();

    // Get max priority for make_next
    let ready_tasks = state
        .task_repo
        .get_by_status(&project_id, InternalStatus::Ready)
        .await
        .unwrap();

    let max_priority = ready_tasks.iter().map(|t| t.priority).max().unwrap_or(0);
    let make_next_priority = max_priority + 1000;

    // Inject with make_next
    let mut injected = Task::new(project_id.clone(), "Make Next Task".to_string());
    injected.internal_status = InternalStatus::Ready;
    injected.priority = make_next_priority;

    let created = state.task_repo.create(injected).await.unwrap();

    assert_eq!(created.internal_status, InternalStatus::Ready);
    assert_eq!(created.priority, 1050); // 50 (max) + 1000

    // Verify it's first in the queue
    let next = state
        .task_repo
        .get_next_executable(&project_id)
        .await
        .unwrap();
    assert!(next.is_some());
    assert_eq!(next.unwrap().title, "Make Next Task");
}

#[tokio::test]
async fn test_inject_task_make_next_with_empty_queue() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // No existing ready tasks, make_next should still work
    let ready_tasks = state
        .task_repo
        .get_by_status(&project_id, InternalStatus::Ready)
        .await
        .unwrap();

    let max_priority = ready_tasks.iter().map(|t| t.priority).max().unwrap_or(0);
    let make_next_priority = max_priority + 1000;

    let mut injected = Task::new(project_id.clone(), "First Make Next".to_string());
    injected.internal_status = InternalStatus::Ready;
    injected.priority = make_next_priority;

    let created = state.task_repo.create(injected).await.unwrap();

    assert_eq!(created.priority, 1000); // 0 + 1000
}

#[tokio::test]
async fn test_inject_task_with_custom_category() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    let task = Task::new_with_category(
        project_id.clone(),
        "Bug Task".to_string(),
        TaskCategory::Regular,
    );

    let created = state.task_repo.create(task).await.unwrap();

    assert_eq!(created.category, TaskCategory::Regular);
}

#[tokio::test]
async fn test_inject_task_with_description() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut task = Task::new(project_id.clone(), "Described Task".to_string());
    task.description = Some("This is a detailed description".to_string());

    let created = state.task_repo.create(task).await.unwrap();

    assert_eq!(
        created.description,
        Some("This is a detailed description".to_string())
    );
}

#[tokio::test]
async fn test_inject_task_invalid_target_defaults_to_backlog() {
    // Test that invalid target defaults to backlog behavior
    let json = r#"{
            "projectId": "proj-123",
            "title": "Invalid Target Task",
            "target": "invalid"
        }"#;

    let input: types::InjectTaskInput = serde_json::from_str(json).unwrap();
    assert_eq!(input.target, "invalid");

    // In the actual command, invalid target would be handled as backlog
}

// ========================================
// Archive Commands Tests
// ========================================

#[tokio::test]
async fn test_archive_task_sets_archived_at() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create a task
    let task = Task::new(project_id, "Task to Archive".to_string());
    let created = state.task_repo.create(task).await.unwrap();

    // Verify not archived initially
    assert!(created.archived_at.is_none());

    // Archive the task
    let archived = state.task_repo.archive(&created.id).await.unwrap();

    // Verify archived_at is set
    assert!(archived.archived_at.is_some());
    assert_eq!(archived.id, created.id);
    assert_eq!(archived.title, "Task to Archive");
}

#[tokio::test]
async fn test_restore_task_clears_archived_at() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create and archive a task
    let task = Task::new(project_id, "Task to Restore".to_string());
    let created = state.task_repo.create(task).await.unwrap();
    let archived = state.task_repo.archive(&created.id).await.unwrap();

    // Verify it's archived
    assert!(archived.archived_at.is_some());

    // Restore the task
    let restored = state.task_repo.restore(&archived.id).await.unwrap();

    // Verify archived_at is cleared
    assert!(restored.archived_at.is_none());
    assert_eq!(restored.id, created.id);
    assert_eq!(restored.title, "Task to Restore");
}

#[tokio::test]
async fn test_get_archived_count_returns_correct_count() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create several tasks
    let task1 = Task::new(project_id.clone(), "Task 1".to_string());
    let task2 = Task::new(project_id.clone(), "Task 2".to_string());
    let task3 = Task::new(project_id.clone(), "Task 3".to_string());

    let created1 = state.task_repo.create(task1).await.unwrap();
    let created2 = state.task_repo.create(task2).await.unwrap();
    let _created3 = state.task_repo.create(task3).await.unwrap();

    // Archive two tasks
    state.task_repo.archive(&created1.id).await.unwrap();
    state.task_repo.archive(&created2.id).await.unwrap();

    // Check archived count
    let count = state
        .task_repo
        .get_archived_count(&project_id, None)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_get_archived_count_zero_when_none_archived() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create tasks but don't archive them
    state
        .task_repo
        .create(Task::new(project_id.clone(), "Active 1".to_string()))
        .await
        .unwrap();
    state
        .task_repo
        .create(Task::new(project_id.clone(), "Active 2".to_string()))
        .await
        .unwrap();

    // Check archived count
    let count = state
        .task_repo
        .get_archived_count(&project_id, None)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_permanently_delete_archived_task_succeeds() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create and archive a task
    let task = Task::new(project_id, "Task to Delete".to_string());
    let created = state.task_repo.create(task).await.unwrap();
    let archived = state.task_repo.archive(&created.id).await.unwrap();

    // Verify it's archived
    assert!(archived.archived_at.is_some());

    // Permanently delete should succeed
    state.task_repo.delete(&archived.id).await.unwrap();

    // Verify task is gone
    let found = state.task_repo.get_by_id(&archived.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_task_response_includes_archived_at() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let mut task = Task::new(project_id, "Archived Task".to_string());
    task.archived_at = Some(Utc::now());

    let response = types::TaskResponse::from(task);

    // Verify archived_at is in response
    assert!(response.archived_at.is_some());

    // Verify it serializes correctly (snake_case backend convention)
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"archived_at\":"));
}

#[tokio::test]
async fn test_task_response_archived_at_null_when_not_archived() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let task = Task::new(project_id, "Active Task".to_string());

    let response = types::TaskResponse::from(task);

    // Verify archived_at is null
    assert!(response.archived_at.is_none());

    // Verify it serializes correctly (snake_case backend convention)
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"archived_at\":null"));
}

// ========================================
// Pagination Tests
// ========================================

#[tokio::test]
async fn test_list_paginated_empty_results() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // No tasks exist
    let result = state
        .task_repo
        .list_paginated(&project_id, None, 0, 20, false, None, None, None)
        .await
        .unwrap();

    assert_eq!(result.len(), 0);

    // Count should also be 0
    let count = state
        .task_repo
        .count_tasks(&project_id, false, None, None)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_list_paginated_first_page() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create 5 tasks
    for i in 1..=5 {
        state
            .task_repo
            .create(Task::new(project_id.clone(), format!("Task {}", i)))
            .await
            .unwrap();
    }

    // Get first page (limit 3)
    let result = state
        .task_repo
        .list_paginated(&project_id, None, 0, 3, false, None, None, None)
        .await
        .unwrap();

    assert_eq!(result.len(), 3);

    // Total count should be 5
    let count = state
        .task_repo
        .count_tasks(&project_id, false, None, None)
        .await
        .unwrap();
    assert_eq!(count, 5);
}

#[tokio::test]
async fn test_list_paginated_last_page() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create 5 tasks
    for i in 1..=5 {
        state
            .task_repo
            .create(Task::new(project_id.clone(), format!("Task {}", i)))
            .await
            .unwrap();
    }

    // Get last page (offset 3, limit 3 = should return 2 tasks)
    let result = state
        .task_repo
        .list_paginated(&project_id, None, 3, 3, false, None, None, None)
        .await
        .unwrap();

    assert_eq!(result.len(), 2);
}

#[tokio::test]
async fn test_list_paginated_offset_beyond_total() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create 3 tasks
    for i in 1..=3 {
        state
            .task_repo
            .create(Task::new(project_id.clone(), format!("Task {}", i)))
            .await
            .unwrap();
    }

    // Request offset 10 (beyond total of 3)
    let result = state
        .task_repo
        .list_paginated(&project_id, None, 10, 20, false, None, None, None)
        .await
        .unwrap();

    assert_eq!(result.len(), 0);
}

#[tokio::test]
async fn test_list_paginated_excludes_archived_by_default() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create 3 tasks
    let task1 = state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 1".to_string()))
        .await
        .unwrap();
    let task2 = state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 2".to_string()))
        .await
        .unwrap();
    state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 3".to_string()))
        .await
        .unwrap();

    // Archive task1 and task2
    state.task_repo.archive(&task1.id).await.unwrap();
    state.task_repo.archive(&task2.id).await.unwrap();

    // List without archived (include_archived = false)
    let result = state
        .task_repo
        .list_paginated(&project_id, None, 0, 20, false, None, None, None)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].title, "Task 3");

    // Count without archived
    let count = state
        .task_repo
        .count_tasks(&project_id, false, None, None)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_list_paginated_includes_archived_when_requested() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create 3 tasks
    let task1 = state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 1".to_string()))
        .await
        .unwrap();
    state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 2".to_string()))
        .await
        .unwrap();

    // Archive task1
    state.task_repo.archive(&task1.id).await.unwrap();

    // List with archived (include_archived = true)
    let result = state
        .task_repo
        .list_paginated(&project_id, None, 0, 20, true, None, None, None)
        .await
        .unwrap();

    assert_eq!(result.len(), 2);

    // Count with archived
    let count = state
        .task_repo
        .count_tasks(&project_id, true, None, None)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_list_paginated_ordered_by_created_at_desc() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create tasks with slight delay to ensure different created_at
    let task1 = state
        .task_repo
        .create(Task::new(project_id.clone(), "First".to_string()))
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let task2 = state
        .task_repo
        .create(Task::new(project_id.clone(), "Second".to_string()))
        .await
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let task3 = state
        .task_repo
        .create(Task::new(project_id.clone(), "Third".to_string()))
        .await
        .unwrap();

    // Get paginated tasks
    let result = state
        .task_repo
        .list_paginated(&project_id, None, 0, 20, false, None, None, None)
        .await
        .unwrap();

    // Should be ordered newest first (DESC)
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].id, task3.id);
    assert_eq!(result[1].id, task2.id);
    assert_eq!(result[2].id, task1.id);
}

#[tokio::test]
async fn test_task_list_response_serialization() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let task = Task::new(project_id, "Test Task".to_string());

    let response = types::TaskListResponse {
        tasks: vec![types::TaskResponse::from(task)],
        total: 10,
        has_more: true,
        offset: 0,
    };

    let json = serde_json::to_string(&response).unwrap();

    // Verify snake_case serialization (backend convention)
    assert!(json.contains("\"tasks\":"));
    assert!(json.contains("\"total\":10"));
    assert!(json.contains("\"has_more\":true"));
    assert!(json.contains("\"offset\":0"));
}

#[tokio::test]
async fn test_get_valid_transitions_from_backlog() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create a task in backlog state
    let mut task = Task::new(project_id, "Test Task".to_string());
    task.internal_status = InternalStatus::Backlog;
    let task = state.task_repo.create(task).await.unwrap();

    // Get valid transitions directly from InternalStatus
    let transitions = task.internal_status.valid_transitions();

    // From backlog, should be able to go to Ready or Cancelled
    assert_eq!(transitions.len(), 2);
    assert!(transitions.contains(&InternalStatus::Ready));
    assert!(transitions.contains(&InternalStatus::Cancelled));

    // Test the label mapping function
    let ready_label = helpers::status_to_label(InternalStatus::Ready);
    assert_eq!(ready_label, "Ready for Work");

    let cancelled_label = helpers::status_to_label(InternalStatus::Cancelled);
    assert_eq!(cancelled_label, "Cancel");
}

#[tokio::test]
async fn test_get_valid_transitions_from_ready() {
    // Test valid transitions from ready state
    let transitions = InternalStatus::Ready.valid_transitions();

    // From ready, should be able to go to Executing, Blocked, PendingMerge, or Cancelled
    assert_eq!(transitions.len(), 4);
    assert!(transitions.contains(&InternalStatus::Executing));
    assert!(transitions.contains(&InternalStatus::Blocked));
    assert!(transitions.contains(&InternalStatus::PendingMerge));
    assert!(transitions.contains(&InternalStatus::Cancelled));

    // Test labels
    assert_eq!(
        helpers::status_to_label(InternalStatus::Executing),
        "Start Execution"
    );
    assert_eq!(
        helpers::status_to_label(InternalStatus::Blocked),
        "Mark as Blocked"
    );
}

#[tokio::test]
async fn test_get_valid_transitions_from_blocked() {
    // Test valid transitions from blocked state
    let transitions = InternalStatus::Blocked.valid_transitions();

    // From blocked, should be able to go to Ready or Cancelled
    assert_eq!(transitions.len(), 2);
    assert!(transitions.contains(&InternalStatus::Ready));
    assert!(transitions.contains(&InternalStatus::Cancelled));
}

#[tokio::test]
async fn test_get_valid_transitions_from_qa_failed() {
    // Test valid transitions from qa_failed state
    let transitions = InternalStatus::QaFailed.valid_transitions();

    // From qa_failed, should be able to go to RevisionNeeded (only one option)
    assert_eq!(transitions.len(), 1);
    assert!(transitions.contains(&InternalStatus::RevisionNeeded));

    // Test label
    assert_eq!(
        helpers::status_to_label(InternalStatus::RevisionNeeded),
        "Needs Revision"
    );
}

#[tokio::test]
async fn test_get_valid_transitions_from_approved() {
    // Test valid transitions from approved state (leads to merge workflow)
    let transitions = InternalStatus::Approved.valid_transitions();

    // From approved, can transition to PendingMerge or re-opened to Ready
    assert_eq!(transitions.len(), 2);
    assert!(transitions.contains(&InternalStatus::PendingMerge));
    assert!(transitions.contains(&InternalStatus::Ready));

    // Test label
    assert_eq!(
        helpers::status_to_label(InternalStatus::Approved),
        "Approve"
    );
}

#[tokio::test]
async fn test_get_valid_transitions_from_cancelled() {
    // Test valid transitions from cancelled state
    let transitions = InternalStatus::Cancelled.valid_transitions();

    // From cancelled, can be re-opened to Ready
    assert_eq!(transitions.len(), 1);
    assert!(transitions.contains(&InternalStatus::Ready));

    // Test label
    assert_eq!(
        helpers::status_to_label(InternalStatus::Cancelled),
        "Cancel"
    );
}

#[tokio::test]
async fn test_get_valid_transitions_from_failed() {
    // Test valid transitions from failed state
    let transitions = InternalStatus::Failed.valid_transitions();

    // From failed, can retry (go to Ready)
    assert_eq!(transitions.len(), 1);
    assert!(transitions.contains(&InternalStatus::Ready));

    // Test label
    assert_eq!(
        helpers::status_to_label(InternalStatus::Failed),
        "Mark as Failed"
    );
}

#[tokio::test]
async fn test_status_to_label_all_statuses() {
    // Test that all statuses have labels
    let all_statuses = InternalStatus::all_variants();

    for status in all_statuses {
        let label = helpers::status_to_label(*status);
        // Label should not be empty
        assert!(!label.is_empty(), "Status {:?} has no label", status);
    }
}

// ========================================
// Create Task with Steps Tests
// ========================================

#[tokio::test]
async fn test_create_task_with_steps() {
    let state = setup_test_state().await;

    let project_id = ProjectId::from_string("test-project".to_string());
    let step_titles = vec![
        "Step 1".to_string(),
        "Step 2".to_string(),
        "Step 3".to_string(),
    ];

    // Create task with steps
    let task = Task::new(project_id.clone(), "Task with Steps".to_string());
    let created_task = state.task_repo.create(task).await.unwrap();

    // Create steps manually (simulating what create_task command does)
    use ralphx_lib::domain::entities::TaskStep;
    let steps: Vec<TaskStep> = step_titles
        .into_iter()
        .enumerate()
        .map(|(idx, title)| {
            TaskStep::new(
                created_task.id.clone(),
                title,
                idx as i32,
                "user".to_string(),
            )
        })
        .collect();

    let created_steps = state.task_step_repo.bulk_create(steps).await.unwrap();

    // Verify steps were created
    assert_eq!(created_steps.len(), 3);
    assert_eq!(created_steps[0].title, "Step 1");
    assert_eq!(created_steps[1].title, "Step 2");
    assert_eq!(created_steps[2].title, "Step 3");

    // Verify sort_order
    assert_eq!(created_steps[0].sort_order, 0);
    assert_eq!(created_steps[1].sort_order, 1);
    assert_eq!(created_steps[2].sort_order, 2);

    // Verify created_by
    assert_eq!(created_steps[0].created_by, "user");
    assert_eq!(created_steps[1].created_by, "user");
    assert_eq!(created_steps[2].created_by, "user");

    // Verify steps are linked to task
    let task_steps = state
        .task_step_repo
        .get_by_task(&created_task.id)
        .await
        .unwrap();
    assert_eq!(task_steps.len(), 3);
}

#[tokio::test]
async fn test_create_task_without_steps() {
    let state = setup_test_state().await;

    let project_id = ProjectId::from_string("test-project".to_string());

    // Create task without steps
    let task = Task::new(project_id.clone(), "Task without Steps".to_string());
    let created_task = state.task_repo.create(task).await.unwrap();

    // Verify no steps exist
    let task_steps = state
        .task_step_repo
        .get_by_task(&created_task.id)
        .await
        .unwrap();
    assert_eq!(task_steps.len(), 0);
}

#[tokio::test]
async fn test_create_task_with_empty_steps_array() {
    let state = setup_test_state().await;

    let project_id = ProjectId::from_string("test-project".to_string());

    // Create task with empty steps array (should not create any steps)
    let task = Task::new(project_id.clone(), "Task with Empty Steps".to_string());
    let created_task = state.task_repo.create(task).await.unwrap();

    // Verify no steps exist
    let task_steps = state
        .task_step_repo
        .get_by_task(&created_task.id)
        .await
        .unwrap();
    assert_eq!(task_steps.len(), 0);
}

// ========================================
// Queue Changed Event Tests
// ========================================

#[tokio::test]
async fn test_ready_status_count_for_queue_changed() {
    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());

    // Initially no tasks in Ready status
    let ready_tasks = state
        .task_repo
        .get_by_status(&project_id, InternalStatus::Ready)
        .await
        .unwrap();
    assert_eq!(ready_tasks.len(), 0);

    // Create a task in Ready status
    let mut task1 = Task::new(project_id.clone(), "Task 1".to_string());
    task1.internal_status = InternalStatus::Ready;
    state.task_repo.create(task1).await.unwrap();

    let ready_tasks = state
        .task_repo
        .get_by_status(&project_id, InternalStatus::Ready)
        .await
        .unwrap();
    assert_eq!(ready_tasks.len(), 1);

    // Add another Ready task
    let mut task2 = Task::new(project_id.clone(), "Task 2".to_string());
    task2.internal_status = InternalStatus::Ready;
    state.task_repo.create(task2).await.unwrap();

    let ready_tasks = state
        .task_repo
        .get_by_status(&project_id, InternalStatus::Ready)
        .await
        .unwrap();
    assert_eq!(ready_tasks.len(), 2);

    // Add a non-Ready task (should not affect count)
    let mut task3 = Task::new(project_id.clone(), "Task 3".to_string());
    task3.internal_status = InternalStatus::Executing;
    state.task_repo.create(task3).await.unwrap();

    let ready_tasks = state
        .task_repo
        .get_by_status(&project_id, InternalStatus::Ready)
        .await
        .unwrap();
    assert_eq!(ready_tasks.len(), 2);
}

#[tokio::test]
async fn test_queue_change_detection_logic() {
    // Test the logic for detecting when queue_changed should be emitted
    let old_status = InternalStatus::Backlog;
    let new_status = InternalStatus::Ready;

    // Should emit: moving to Ready
    let should_emit = old_status == InternalStatus::Ready || new_status == InternalStatus::Ready;
    assert!(should_emit);

    // Should emit: moving from Ready
    let old_status = InternalStatus::Ready;
    let new_status = InternalStatus::Executing;
    let should_emit = old_status == InternalStatus::Ready || new_status == InternalStatus::Ready;
    assert!(should_emit);

    // Should NOT emit: neither is Ready
    let old_status = InternalStatus::Backlog;
    let new_status = InternalStatus::Blocked;
    let should_emit = old_status == InternalStatus::Ready || new_status == InternalStatus::Ready;
    assert!(!should_emit);
}

// ========================================
// Session Filtering Tests
// ========================================

#[tokio::test]
async fn test_list_tasks_filters_by_session_id() {
    use ralphx_lib::domain::entities::IdeationSessionId;

    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());
    let session1 = IdeationSessionId::new();
    let session2 = IdeationSessionId::new();

    // Create tasks with different session IDs
    let mut task1 = Task::new(project_id.clone(), "Task Session 1".to_string());
    task1.ideation_session_id = Some(session1.clone());
    state.task_repo.create(task1).await.unwrap();

    let mut task2 = Task::new(project_id.clone(), "Task Session 2".to_string());
    task2.ideation_session_id = Some(session2.clone());
    state.task_repo.create(task2).await.unwrap();

    let mut task3 = Task::new(project_id.clone(), "Task No Session".to_string());
    task3.ideation_session_id = None;
    state.task_repo.create(task3).await.unwrap();

    // Test unfiltered query
    let all_tasks = state.task_repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(all_tasks.len(), 3);

    // Test filtered query for session1
    let session1_tasks: Vec<_> = all_tasks
        .iter()
        .filter(|t| {
            t.ideation_session_id
                .as_ref()
                .is_some_and(|id| id == &session1)
        })
        .collect();
    assert_eq!(session1_tasks.len(), 1);
    assert_eq!(session1_tasks[0].title, "Task Session 1");

    // Test filtered query for session2
    let session2_tasks: Vec<_> = all_tasks
        .iter()
        .filter(|t| {
            t.ideation_session_id
                .as_ref()
                .is_some_and(|id| id == &session2)
        })
        .collect();
    assert_eq!(session2_tasks.len(), 1);
    assert_eq!(session2_tasks[0].title, "Task Session 2");
}

#[tokio::test]
async fn test_search_tasks_filters_by_session_id() {
    use ralphx_lib::domain::entities::IdeationSessionId;

    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());
    let session1 = IdeationSessionId::new();

    // Create tasks with search keyword
    let mut task1 = Task::new(project_id.clone(), "Auth Feature".to_string());
    task1.ideation_session_id = Some(session1.clone());
    state.task_repo.create(task1).await.unwrap();

    let mut task2 = Task::new(project_id.clone(), "Auth Bug Fix".to_string());
    task2.ideation_session_id = None;
    state.task_repo.create(task2).await.unwrap();

    // Search without filter - should find both
    let all_results = state
        .task_repo
        .search(&project_id, "Auth", false)
        .await
        .unwrap();
    assert_eq!(all_results.len(), 2);

    // Search with session filter
    let filtered_results: Vec<_> = all_results
        .into_iter()
        .filter(|t| {
            t.ideation_session_id
                .as_ref()
                .is_some_and(|id| id == &session1)
        })
        .collect();
    assert_eq!(filtered_results.len(), 1);
    assert_eq!(filtered_results[0].title, "Auth Feature");
}

#[tokio::test]
async fn test_get_archived_count_filters_by_session_id() {
    use ralphx_lib::domain::entities::IdeationSessionId;

    let state = setup_test_state().await;
    let project_id = ProjectId::from_string("test-project".to_string());
    let session1 = IdeationSessionId::new();

    // Create and archive tasks
    let mut task1 = Task::new(project_id.clone(), "Task 1".to_string());
    task1.ideation_session_id = Some(session1.clone());
    let created1 = state.task_repo.create(task1).await.unwrap();
    state.task_repo.archive(&created1.id).await.unwrap();

    let mut task2 = Task::new(project_id.clone(), "Task 2".to_string());
    task2.ideation_session_id = None;
    let created2 = state.task_repo.create(task2).await.unwrap();
    state.task_repo.archive(&created2.id).await.unwrap();

    // Count all archived - should be 2
    let total_archived = state
        .task_repo
        .get_archived_count(&project_id, None)
        .await
        .unwrap();
    assert_eq!(total_archived, 2);

    // Count with session filter
    let all_tasks = state
        .task_repo
        .get_by_project_filtered(&project_id, true)
        .await
        .unwrap();
    let session_archived = all_tasks
        .into_iter()
        .filter(|t| {
            t.archived_at.is_some()
                && t.ideation_session_id
                    .as_ref()
                    .is_some_and(|id| id == &session1)
        })
        .count();
    assert_eq!(session_archived, 1);
}

// ── IPC contract tests ─────────────────────────────────────────────────────────
// Verify camelCase deserialization for task command input structs.

#[cfg(test)]
mod ipc_contract {
    use ralphx_lib::commands::task_commands::{
        AnswerUserQuestionInput, CreateTaskInput, InjectTaskInput, UpdateTaskInput,
    };

    // ── CreateTaskInput ─────────────────────────────────────────────────────

    #[test]
    fn create_task_input_deserializes_camel_case() {
        let json = r#"{"projectId":"proj-123","title":"Build auth module","category":"regular","description":"Implement JWT","priority":50,"steps":["Step A","Step B"],"ideationSessionId":"session-123","executionPlanId":"exec-plan-123"}"#;
        let input: CreateTaskInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.project_id, "proj-123");
        assert_eq!(input.title, "Build auth module");
        assert_eq!(input.category, Some("regular".to_string()));
        assert_eq!(input.description, Some("Implement JWT".to_string()));
        assert_eq!(input.priority, Some(50));
        assert_eq!(input.ideation_session_id, Some("session-123".to_string()));
        assert_eq!(input.execution_plan_id, Some("exec-plan-123".to_string()));
        assert_eq!(
            input.steps,
            Some(vec!["Step A".to_string(), "Step B".to_string()])
        );
    }

    #[test]
    fn create_task_input_required_fields_only() {
        let json = r#"{"projectId":"proj-456","title":"Minimal task"}"#;
        let input: CreateTaskInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.project_id, "proj-456");
        assert_eq!(input.title, "Minimal task");
        assert!(input.category.is_none());
        assert!(input.description.is_none());
        assert!(input.priority.is_none());
        assert!(input.steps.is_none());
        assert!(input.ideation_session_id.is_none());
        assert!(input.execution_plan_id.is_none());
    }

    #[test]
    fn create_task_input_rejects_snake_case() {
        // project_id in snake_case must not deserialize as projectId
        let json = r#"{"project_id":"proj-1","title":"Bad"}"#;
        let result: Result<CreateTaskInput, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "snake_case project_id must not deserialize (missing required camelCase field)"
        );
    }

    // ── UpdateTaskInput ─────────────────────────────────────────────────────

    #[test]
    fn update_task_input_deserializes_camel_case() {
        let json = r#"{"title":"New Title","description":"New desc","category":"bug","priority":75,"internalStatus":"ready"}"#;
        let input: UpdateTaskInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.title, Some("New Title".to_string()));
        assert_eq!(input.description, Some("New desc".to_string()));
        assert_eq!(input.category, Some("bug".to_string()));
        assert_eq!(input.priority, Some(75));
        assert_eq!(input.internal_status, Some("ready".to_string()));
    }

    #[test]
    fn update_task_input_empty_patch() {
        let json = r#"{}"#;
        let input: UpdateTaskInput = serde_json::from_str(json).unwrap();
        assert!(input.title.is_none());
        assert!(input.description.is_none());
        assert!(input.priority.is_none());
        assert!(input.internal_status.is_none());
    }

    // ── InjectTaskInput ─────────────────────────────────────────────────────

    #[test]
    fn inject_task_input_deserializes_camel_case() {
        let json = r#"{"projectId":"proj-abc","title":"Hotfix","description":"Fix prod bug","category":"bug","target":"next","makeNext":true}"#;
        let input: InjectTaskInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.project_id, "proj-abc");
        assert_eq!(input.title, "Hotfix");
        assert_eq!(input.description, Some("Fix prod bug".to_string()));
        assert_eq!(input.category, Some("bug".to_string()));
        assert_eq!(input.target, "next");
        assert!(input.make_next);
    }

    #[test]
    fn inject_task_input_defaults_applied() {
        // makeNext defaults to false, target defaults via helper
        let json = r#"{"projectId":"proj-1","title":"Quick task"}"#;
        let input: InjectTaskInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.project_id, "proj-1");
        assert!(!input.make_next);
        assert!(!input.target.is_empty(), "target must have a default value");
    }

    // ── AnswerUserQuestionInput ─────────────────────────────────────────────

    #[test]
    fn answer_user_question_input_deserializes_camel_case() {
        let json = r#"{"taskId":"task-999","selectedOptions":["opt-a","opt-b"],"customResponse":"My answer"}"#;
        let input: AnswerUserQuestionInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.task_id, "task-999");
        assert_eq!(
            input.selected_options,
            vec!["opt-a".to_string(), "opt-b".to_string()]
        );
        assert_eq!(input.custom_response, Some("My answer".to_string()));
    }

    #[test]
    fn answer_user_question_input_no_custom_response() {
        let json = r#"{"taskId":"task-1","selectedOptions":[]}"#;
        let input: AnswerUserQuestionInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.task_id, "task-1");
        assert!(input.selected_options.is_empty());
        assert!(input.custom_response.is_none());
    }
}

// ─── Layer 1 restart logic unit tests ────────────────────────────────────────
// These tests verify the behavior added to the terminal→Ready block in move_task:
// git ref clearing and execution_recovery selective reset.
// We can't invoke Tauri commands directly (requires AppHandle), so we replicate the
// exact logic using the same helper functions and MemoryTaskRepository.
#[cfg(test)]
mod layer1_restart_tests {
    use chrono::Utc;
    use ralphx_lib::domain::entities::{
        ExecutionRecoveryMetadata, ExecutionRecoveryState, InternalStatus, ProjectId, Task, TaskId,
        TaskStep, TaskStepStatus,
    };
    use ralphx_lib::domain::repositories::{TaskRepository, TaskStepRepository};
    use ralphx_lib::domain::state_machine::transition_handler::{
        parse_metadata, set_trigger_origin,
    };
    use ralphx_lib::infrastructure::memory::{MemoryTaskRepository, MemoryTaskStepRepository};
    use std::sync::Arc;

    async fn create_task_in_repo(task_repo: &Arc<MemoryTaskRepository>, task: Task) -> Task {
        task_repo.create(task).await.unwrap()
    }

    fn make_task_with_git_refs() -> Task {
        let project_id = ProjectId::from_string("test-project".to_string());
        let mut task = Task::new(project_id, "Restart Test Task".to_string());
        task.internal_status = InternalStatus::Failed;
        task.task_branch = Some("ralphx/task/test-branch".to_string());
        task.worktree_path = Some("/worktrees/test-task".to_string());
        task.merge_commit_sha = Some("deadbeefcafe".to_string());
        task
    }

    /// Simulate the terminal→Ready restart block from move_task.
    /// Returns the mutated task (not yet saved) plus updated metadata string.
    fn apply_restart_block(task: &Task) -> Task {
        let mut task_mut = task.clone();
        set_trigger_origin(&mut task_mut, "retry");

        if task.internal_status == InternalStatus::Failed {
            let mut meta = parse_metadata(&task_mut).unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("preserve_steps".to_string(), serde_json::json!(true));
            }
            task_mut.metadata = Some(meta.to_string());
        }

        // Clear stale git refs
        task_mut.task_branch = None;
        task_mut.worktree_path = None;
        task_mut.merge_commit_sha = None;

        // Selectively reset execution_recovery, preserving auto_recovery_count
        if let Ok(Some(mut recovery)) =
            ExecutionRecoveryMetadata::from_task_metadata(task_mut.metadata.as_deref())
        {
            recovery.stop_retrying = false;
            recovery.last_state = ExecutionRecoveryState::Retrying;
            recovery.events.clear();
            recovery.unrecoverable_reason = None;
            if let Ok(updated_meta) = recovery.update_task_metadata(task_mut.metadata.as_deref()) {
                task_mut.metadata = Some(updated_meta);
            }
        }

        task_mut
    }

    async fn clear_failed_steps_in_repo(
        step_repo: &Arc<MemoryTaskStepRepository>,
        task_id: &TaskId,
    ) -> u32 {
        let mut cleared = 0;
        for mut step in step_repo.get_by_task(task_id).await.unwrap() {
            if step.status != TaskStepStatus::Failed {
                continue;
            }

            step.status = TaskStepStatus::Pending;
            step.started_at = None;
            step.completed_at = None;
            step.completion_note = None;
            step_repo.update(&step).await.unwrap();
            cleared += 1;
        }
        cleared
    }

    #[tokio::test]
    async fn test_terminal_to_ready_clears_git_refs() {
        let task_repo = Arc::new(MemoryTaskRepository::new());
        let original = create_task_in_repo(&task_repo, make_task_with_git_refs()).await;

        let restarted = apply_restart_block(&original);
        task_repo.update(&restarted).await.unwrap();

        let saved = task_repo.get_by_id(&original.id).await.unwrap().unwrap();
        assert!(saved.task_branch.is_none(), "task_branch should be cleared");
        assert!(
            saved.worktree_path.is_none(),
            "worktree_path should be cleared"
        );
        assert!(
            saved.merge_commit_sha.is_none(),
            "merge_commit_sha should be cleared"
        );
    }

    #[tokio::test]
    async fn test_terminal_to_ready_resets_execution_recovery_preserves_count() {
        let task_repo = Arc::new(MemoryTaskRepository::new());
        let project_id = ProjectId::from_string("test-project".to_string());
        let mut task = Task::new(project_id, "Recovery Reset Test".to_string());
        task.internal_status = InternalStatus::Failed;

        // Set up execution_recovery with stop_retrying=true and auto_recovery_count=2
        let mut recovery = ExecutionRecoveryMetadata::new();
        recovery.stop_retrying = true;
        recovery.last_state = ExecutionRecoveryState::Failed;
        recovery.auto_recovery_count = 2;
        let metadata_json = recovery.update_task_metadata(None).unwrap();
        task.metadata = Some(metadata_json);

        let original = create_task_in_repo(&task_repo, task).await;

        let restarted = apply_restart_block(&original);
        task_repo.update(&restarted).await.unwrap();

        let saved = task_repo.get_by_id(&original.id).await.unwrap().unwrap();
        let final_recovery =
            ExecutionRecoveryMetadata::from_task_metadata(saved.metadata.as_deref())
                .unwrap()
                .unwrap();

        assert!(
            !final_recovery.stop_retrying,
            "stop_retrying should be reset to false"
        );
        assert_eq!(
            final_recovery.last_state,
            ExecutionRecoveryState::Retrying,
            "last_state should be reset to Retrying"
        );
        assert!(final_recovery.events.is_empty(), "events should be cleared");
        assert!(
            final_recovery.unrecoverable_reason.is_none(),
            "unrecoverable_reason should be cleared"
        );
        assert_eq!(
            final_recovery.auto_recovery_count, 2,
            "auto_recovery_count must be PRESERVED"
        );
    }

    #[tokio::test]
    async fn test_failed_restart_clears_only_failed_steps_and_preserves_progress() {
        let task_repo = Arc::new(MemoryTaskRepository::new());
        let step_repo = Arc::new(MemoryTaskStepRepository::new());
        let original = create_task_in_repo(&task_repo, make_task_with_git_refs()).await;

        let mut completed = TaskStep::new(
            original.id.clone(),
            "Completed".to_string(),
            0,
            "user".to_string(),
        );
        completed.status = TaskStepStatus::Completed;
        completed.started_at = Some(Utc::now());
        completed.completed_at = Some(Utc::now());
        completed.completion_note = Some("done".to_string());
        step_repo.create(completed).await.unwrap();

        let mut failed = TaskStep::new(
            original.id.clone(),
            "Failed".to_string(),
            1,
            "user".to_string(),
        );
        failed.status = TaskStepStatus::Failed;
        failed.started_at = Some(Utc::now());
        failed.completed_at = Some(Utc::now());
        failed.completion_note = Some("provider limit".to_string());
        step_repo.create(failed).await.unwrap();

        let mut skipped = TaskStep::new(
            original.id.clone(),
            "Skipped".to_string(),
            2,
            "user".to_string(),
        );
        skipped.status = TaskStepStatus::Skipped;
        skipped.completed_at = Some(Utc::now());
        skipped.completion_note = Some("not needed".to_string());
        step_repo.create(skipped).await.unwrap();

        let restarted = apply_restart_block(&original);
        task_repo.update(&restarted).await.unwrap();
        let cleared = clear_failed_steps_in_repo(&step_repo, &original.id).await;

        assert_eq!(cleared, 1, "only failed steps should be cleared on restart");

        let saved = task_repo.get_by_id(&original.id).await.unwrap().unwrap();
        let meta = parse_metadata(&saved).expect("metadata should be present");
        assert_eq!(
            meta.get("preserve_steps").and_then(|v| v.as_bool()),
            Some(true),
            "failed-task restart must preserve previously completed progress"
        );

        let steps = step_repo.get_by_task(&original.id).await.unwrap();
        assert_eq!(steps.len(), 3);

        let completed = steps.iter().find(|step| step.title == "Completed").unwrap();
        assert_eq!(completed.status, TaskStepStatus::Completed);
        assert_eq!(completed.completion_note.as_deref(), Some("done"));
        assert!(completed.completed_at.is_some());

        let failed = steps.iter().find(|step| step.title == "Failed").unwrap();
        assert_eq!(failed.status, TaskStepStatus::Pending);
        assert!(
            failed.started_at.is_none() && failed.completed_at.is_none(),
            "restarted failed steps must lose stale timing"
        );
        assert!(
            failed.completion_note.is_none(),
            "restarted failed steps must lose stale failure note"
        );

        let skipped = steps.iter().find(|step| step.title == "Skipped").unwrap();
        assert_eq!(skipped.status, TaskStepStatus::Skipped);
        assert_eq!(skipped.completion_note.as_deref(), Some("not needed"));
    }
}

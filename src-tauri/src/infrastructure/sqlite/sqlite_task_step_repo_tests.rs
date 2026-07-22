use super::*;
use crate::domain::entities::Task;
use crate::domain::ideation::TasksFeatureState;
use crate::domain::repositories::IdeationSettingsRepository;
use crate::infrastructure::sqlite::SqliteIdeationSettingsRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite-task-step-repo")
}

fn create_test_task(db: &SqliteTestDb, task_id: &TaskId) {
    let project = db.seed_project("Test Project");
    let task = Task::new(project.id, "Test Task".to_string());
    let task = Task {
        id: task_id.clone(),
        ..task
    };
    db.insert_task(task);
}

#[tokio::test]
async fn task_step_history_mutation_requires_tasks_enabled() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let shared = db.shared_conn();
    let repo = SqliteTaskStepRepository::from_shared(shared.clone()).with_tasks_feature_policy();
    let rejected_step = TaskStep::new(
        task_id.clone(),
        "Rejected while disabled".to_string(),
        0,
        "user".to_string(),
    );

    let error = repo
        .create(rejected_step)
        .await
        .expect_err("disabled Tasks must reject step writes");
    assert!(matches!(error, AppError::FeatureDisabled(_)));
    assert!(repo.get_by_task(&task_id).await.unwrap().is_empty());

    let settings_repo = SqliteIdeationSettingsRepository::from_shared(shared);
    assert!(settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Disabled,
            TasksFeatureState::Enabled,
        )
        .await
        .unwrap());
    repo.create(TaskStep::new(
        task_id.clone(),
        "Allowed while enabled".to_string(),
        0,
        "user".to_string(),
    ))
    .await
    .expect("enabled Tasks should allow step writes");
    assert_eq!(repo.get_by_task(&task_id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn guarded_step_lifecycle_checks_authority_for_every_history_mutation() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let shared = db.shared_conn();
    let settings_repo = SqliteIdeationSettingsRepository::from_shared(shared.clone());
    assert!(settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Disabled,
            TasksFeatureState::Enabled,
        )
        .await
        .unwrap());
    let repo = SqliteTaskStepRepository::from_shared(shared).with_tasks_feature_policy();

    let parent = repo
        .create(TaskStep::new(
            task_id.clone(),
            "Parent".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();
    let mut child = repo
        .create(TaskStep::new(
            task_id.clone(),
            "Child".to_string(),
            1,
            "user".to_string(),
        ))
        .await
        .unwrap();
    child.parent_step_id = Some(parent.id.clone());
    repo.update(&child)
        .await
        .expect("guarded update should preserve the parent relationship");
    assert_eq!(
        repo.get_by_id(&child.id)
            .await
            .unwrap()
            .unwrap()
            .parent_step_id,
        Some(parent.id.clone())
    );

    let bulk = repo
        .bulk_create(vec![
            TaskStep::new(
                task_id.clone(),
                "Bulk one".to_string(),
                2,
                "user".to_string(),
            ),
            TaskStep::new(
                task_id.clone(),
                "Bulk two".to_string(),
                3,
                "user".to_string(),
            ),
        ])
        .await
        .expect("guarded bulk creation should succeed while enabled");
    repo.reorder(
        &task_id,
        vec![
            bulk[1].id.clone(),
            child.id.clone(),
            parent.id.clone(),
            bulk[0].id.clone(),
        ],
    )
    .await
    .expect("guarded reorder should succeed while enabled");

    child.status = TaskStepStatus::Failed;
    repo.update(&child).await.unwrap();
    assert_eq!(repo.reset_all_to_pending(&task_id).await.unwrap(), 1);
    assert_eq!(
        repo.get_by_id(&child.id).await.unwrap().unwrap().status,
        TaskStepStatus::Pending
    );

    repo.delete(&bulk[0].id)
        .await
        .expect("guarded single delete should succeed while enabled");
    assert!(repo.get_by_id(&bulk[0].id).await.unwrap().is_none());
    repo.delete_by_task(&task_id)
        .await
        .expect("guarded task delete should succeed while enabled");
    assert!(repo.get_by_task(&task_id).await.unwrap().is_empty());
}

#[tokio::test]
async fn guarded_step_create_rejects_a_missing_task_without_writing() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    let repo = SqliteTaskStepRepository::from_shared(db.shared_conn()).with_tasks_feature_policy();

    let error = repo
        .create(TaskStep::new(
            task_id.clone(),
            "Orphan step".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .expect_err("a guarded step must belong to a persisted task");

    assert!(matches!(error, AppError::TaskNotFound(id) if id == task_id.as_str()));
    assert!(repo.get_by_task(&task_id).await.unwrap().is_empty());
}

#[tokio::test]
async fn test_create_and_get_by_id() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let step = TaskStep::new(
        task_id.clone(),
        "Test step".to_string(),
        0,
        "user".to_string(),
    );
    let step_id = step.id.clone();

    // Create step
    let created = repo.create(step).await.unwrap();
    assert_eq!(created.title, "Test step");

    // Get by ID
    let fetched = repo.get_by_id(&step_id).await.unwrap();
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().title, "Test step");
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let db = setup_test_db();
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let step_id = TaskStepId::new();
    let result = repo.get_by_id(&step_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_by_task_ordered() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 2, "user".to_string());
    let step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 0, "user".to_string());
    let step3 = TaskStep::new(task_id.clone(), "Step 3".to_string(), 1, "user".to_string());

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();

    let steps = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0].title, "Step 2"); // sort_order 0
    assert_eq!(steps[1].title, "Step 3"); // sort_order 1
    assert_eq!(steps[2].title, "Step 1"); // sort_order 2
}

#[tokio::test]
async fn test_get_by_task_and_status() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let mut step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let mut step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());
    let step3 = TaskStep::new(task_id.clone(), "Step 3".to_string(), 2, "user".to_string());

    step1.status = TaskStepStatus::Completed;
    step2.status = TaskStepStatus::InProgress;

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();

    let completed_steps = repo
        .get_by_task_and_status(&task_id, TaskStepStatus::Completed)
        .await
        .unwrap();
    assert_eq!(completed_steps.len(), 1);
    assert_eq!(completed_steps[0].title, "Step 1");

    let pending_steps = repo
        .get_by_task_and_status(&task_id, TaskStepStatus::Pending)
        .await
        .unwrap();
    assert_eq!(pending_steps.len(), 1);
    assert_eq!(pending_steps[0].title, "Step 3");
}

#[tokio::test]
async fn test_update() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let mut step = TaskStep::new(
        task_id.clone(),
        "Original title".to_string(),
        0,
        "user".to_string(),
    );
    let step_id = step.id.clone();

    repo.create(step.clone()).await.unwrap();

    // Update step
    step.title = "Updated title".to_string();
    step.status = TaskStepStatus::Completed;
    repo.update(&step).await.unwrap();

    // Verify update
    let fetched = repo.get_by_id(&step_id).await.unwrap().unwrap();
    assert_eq!(fetched.title, "Updated title");
    assert_eq!(fetched.status, TaskStepStatus::Completed);
}

#[tokio::test]
async fn test_delete() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let step = TaskStep::new(
        task_id.clone(),
        "Test step".to_string(),
        0,
        "user".to_string(),
    );
    let step_id = step.id.clone();

    repo.create(step).await.unwrap();
    repo.delete(&step_id).await.unwrap();

    let result = repo.get_by_id(&step_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_delete_by_task() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();

    repo.delete_by_task(&task_id).await.unwrap();

    let steps = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(steps.len(), 0);
}

#[tokio::test]
async fn test_count_by_status() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let mut step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let mut step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());
    let mut step3 = TaskStep::new(task_id.clone(), "Step 3".to_string(), 2, "user".to_string());
    let step4 = TaskStep::new(task_id.clone(), "Step 4".to_string(), 3, "user".to_string());

    step1.status = TaskStepStatus::Completed;
    step2.status = TaskStepStatus::Completed;
    step3.status = TaskStepStatus::InProgress;

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();
    repo.create(step4).await.unwrap();

    let counts = repo.count_by_status(&task_id).await.unwrap();
    assert_eq!(counts.get(&TaskStepStatus::Completed), Some(&2));
    assert_eq!(counts.get(&TaskStepStatus::InProgress), Some(&1));
    assert_eq!(counts.get(&TaskStepStatus::Pending), Some(&1));
}

#[tokio::test]
async fn test_bulk_create() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let steps = vec![
        TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string()),
        TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string()),
        TaskStep::new(task_id.clone(), "Step 3".to_string(), 2, "user".to_string()),
    ];

    let created = repo.bulk_create(steps).await.unwrap();
    assert_eq!(created.len(), 3);

    let fetched = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(fetched.len(), 3);
}

#[tokio::test]
async fn test_bulk_create_rollback_on_error() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let step = TaskStep::new(
        task_id.clone(),
        "Existing step".to_string(),
        0,
        "user".to_string(),
    );
    let step_id = step.id.clone();

    // Create a step first
    repo.create(step.clone()).await.unwrap();

    // Try to bulk create with a duplicate ID (should fail and rollback)
    let steps = vec![
        step.clone(), // Duplicate ID
        TaskStep::new(
            task_id.clone(),
            "New step".to_string(),
            1,
            "user".to_string(),
        ),
    ];

    let result = repo.bulk_create(steps).await;
    assert!(result.is_err());

    // Verify only the original step exists
    let fetched = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].id, step_id);
}

#[tokio::test]
async fn test_reorder() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());
    let step3 = TaskStep::new(task_id.clone(), "Step 3".to_string(), 2, "user".to_string());

    let step1_id = step1.id.clone();
    let step2_id = step2.id.clone();
    let step3_id = step3.id.clone();

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();

    // Reorder: step3, step1, step2
    let new_order = vec![step3_id.clone(), step1_id.clone(), step2_id.clone()];
    repo.reorder(&task_id, new_order).await.unwrap();

    let steps = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(steps[0].id, step3_id);
    assert_eq!(steps[1].id, step1_id);
    assert_eq!(steps[2].id, step2_id);
    assert_eq!(steps[0].sort_order, 0);
    assert_eq!(steps[1].sort_order, 1);
    assert_eq!(steps[2].sort_order, 2);
}

#[tokio::test]
async fn test_reorder_rollback_on_error() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());

    let step1_id = step1.id.clone();
    let step2_id = step2.id.clone();

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();

    // Try to reorder with a non-existent step ID
    // Note: SQLite won't error on this - it will just not update the invalid row
    // This is expected behavior - the transaction succeeds but only valid IDs are updated
    let invalid_id = TaskStepId::new();
    let new_order = vec![invalid_id, step1_id.clone(), step2_id.clone()];
    let result = repo.reorder(&task_id, new_order).await;

    // Should succeed (no error) since SQLite doesn't error on UPDATE with no matching rows
    assert!(result.is_ok());

    // Valid steps should be updated to positions 1 and 2
    let steps = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].id, step1_id);
    assert_eq!(steps[0].sort_order, 1);
    assert_eq!(steps[1].id, step2_id);
    assert_eq!(steps[1].sort_order, 2);
}

#[tokio::test]
async fn test_reset_all_to_pending_resets_non_pending_steps() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let mut step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let mut step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());
    let mut step3 = TaskStep::new(task_id.clone(), "Step 3".to_string(), 2, "user".to_string());
    let step4 = TaskStep::new(task_id.clone(), "Step 4".to_string(), 3, "user".to_string());

    step1.status = TaskStepStatus::Completed;
    step2.status = TaskStepStatus::InProgress;
    step3.status = TaskStepStatus::Failed;

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();
    repo.create(step4).await.unwrap();

    let count = repo.reset_all_to_pending(&task_id).await.unwrap();
    assert_eq!(count, 3, "Should reset exactly 3 non-Pending steps");

    let steps = repo.get_by_task(&task_id).await.unwrap();
    for step in &steps {
        assert_eq!(
            step.status,
            TaskStepStatus::Pending,
            "All steps should be Pending"
        );
        assert!(step.started_at.is_none(), "started_at should be cleared");
        assert!(
            step.completed_at.is_none(),
            "completed_at should be cleared"
        );
        assert!(
            step.completion_note.is_none(),
            "completion_note should be cleared"
        );
    }
}

#[tokio::test]
async fn test_reset_all_to_pending_noop_when_all_pending() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();

    let count = repo.reset_all_to_pending(&task_id).await.unwrap();
    assert_eq!(
        count, 0,
        "No steps should be reset when all are already Pending"
    );
}

#[tokio::test]
async fn test_reset_all_to_pending_mixed_statuses() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let pending_step = TaskStep::new(
        task_id.clone(),
        "Pending".to_string(),
        0,
        "user".to_string(),
    );
    let pending_id = pending_step.id.clone();
    let mut completed = TaskStep::new(
        task_id.clone(),
        "Completed".to_string(),
        1,
        "user".to_string(),
    );
    completed.status = TaskStepStatus::Completed;
    let mut cancelled = TaskStep::new(
        task_id.clone(),
        "Cancelled".to_string(),
        2,
        "user".to_string(),
    );
    cancelled.status = TaskStepStatus::Cancelled;

    repo.create(pending_step).await.unwrap();
    repo.create(completed).await.unwrap();
    repo.create(cancelled).await.unwrap();

    let count = repo.reset_all_to_pending(&task_id).await.unwrap();
    assert_eq!(count, 2, "Only non-Pending steps should be reset");

    // Pending step should remain unchanged
    let pending_after = repo.get_by_id(&pending_id).await.unwrap().unwrap();
    assert_eq!(pending_after.status, TaskStepStatus::Pending);

    // All steps now Pending
    let steps = repo.get_by_task(&task_id).await.unwrap();
    for step in &steps {
        assert_eq!(step.status, TaskStepStatus::Pending);
    }
}

#[tokio::test]
async fn test_reset_all_to_pending_preserves_structural_fields() {
    let db = setup_test_db();
    let task_id = TaskId::new();
    create_test_task(&db, &task_id);
    let repo = SqliteTaskStepRepository::new(db.new_connection());

    let mut step = TaskStep::new(
        task_id.clone(),
        "Important Step".to_string(),
        5,
        "user".to_string(),
    );
    step.status = TaskStepStatus::Completed;
    step.description = Some("Step description".to_string());
    step.scope_context = Some(r#"{"files":["src/foo.rs"]}"#.to_string());
    let step_id = step.id.clone();

    repo.create(step).await.unwrap();
    repo.reset_all_to_pending(&task_id).await.unwrap();

    let after = repo.get_by_id(&step_id).await.unwrap().unwrap();
    assert_eq!(after.title, "Important Step", "title preserved");
    assert_eq!(after.sort_order, 5, "sort_order preserved");
    assert_eq!(
        after.description,
        Some("Step description".to_string()),
        "description preserved"
    );
    assert_eq!(
        after.scope_context,
        Some(r#"{"files":["src/foo.rs"]}"#.to_string()),
        "scope_context preserved"
    );
    assert_eq!(after.status, TaskStepStatus::Pending);
}

use super::*;

#[tokio::test]
async fn create_stores_step() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();
    let step = TaskStep::new(
        task_id.clone(),
        "Test step".to_string(),
        0,
        "user".to_string(),
    );

    let created = repo.create(step.clone()).await.unwrap();
    assert_eq!(created.id, step.id);
    assert_eq!(created.title, "Test step");

    let retrieved = repo.get_by_id(&step.id).await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().title, "Test step");
}

#[tokio::test]
async fn get_by_task_filters_and_sorts() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();
    let other_task_id = TaskId::new();

    // Create steps with sort_order 2, 0, 1 to test sorting
    let step1 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 2, "user".to_string());
    let step2 = TaskStep::new(task_id.clone(), "Step 0".to_string(), 0, "user".to_string());
    let step3 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 1, "user".to_string());
    let step4 = TaskStep::new(
        other_task_id,
        "Other step".to_string(),
        0,
        "user".to_string(),
    );

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();
    repo.create(step4).await.unwrap();

    let steps = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0].title, "Step 0");
    assert_eq!(steps[1].title, "Step 1");
    assert_eq!(steps[2].title, "Step 2");
}

#[tokio::test]
async fn get_by_task_and_status_filters_correctly() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();

    let step1 = TaskStep::new(
        task_id.clone(),
        "Pending".to_string(),
        0,
        "user".to_string(),
    );
    let mut step2 = TaskStep::new(
        task_id.clone(),
        "In Progress".to_string(),
        1,
        "user".to_string(),
    );
    step2.status = TaskStepStatus::InProgress;
    let mut step3 = TaskStep::new(
        task_id.clone(),
        "Completed".to_string(),
        2,
        "user".to_string(),
    );
    step3.status = TaskStepStatus::Completed;

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();

    let pending = repo
        .get_by_task_and_status(&task_id, TaskStepStatus::Pending)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].title, "Pending");

    let in_progress = repo
        .get_by_task_and_status(&task_id, TaskStepStatus::InProgress)
        .await
        .unwrap();
    assert_eq!(in_progress.len(), 1);
    assert_eq!(in_progress[0].title, "In Progress");
}

#[tokio::test]
async fn update_modifies_step() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();
    let mut step = TaskStep::new(task_id, "Original".to_string(), 0, "user".to_string());

    repo.create(step.clone()).await.unwrap();

    step.title = "Updated".to_string();
    step.status = TaskStepStatus::Completed;
    repo.update(&step).await.unwrap();

    let retrieved = repo.get_by_id(&step.id).await.unwrap().unwrap();
    assert_eq!(retrieved.title, "Updated");
    assert_eq!(retrieved.status, TaskStepStatus::Completed);
}

#[tokio::test]
async fn delete_removes_step() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();
    let step = TaskStep::new(task_id, "Test".to_string(), 0, "user".to_string());

    repo.create(step.clone()).await.unwrap();
    assert!(repo.get_by_id(&step.id).await.unwrap().is_some());

    repo.delete(&step.id).await.unwrap();
    assert!(repo.get_by_id(&step.id).await.unwrap().is_none());
}

#[tokio::test]
async fn delete_by_task_removes_all_steps() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();
    let other_task_id = TaskId::new();

    let step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());
    let step3 = TaskStep::new(
        other_task_id.clone(),
        "Other".to_string(),
        0,
        "user".to_string(),
    );

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();

    repo.delete_by_task(&task_id).await.unwrap();

    let task_steps = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(task_steps.len(), 0);

    let other_steps = repo.get_by_task(&other_task_id).await.unwrap();
    assert_eq!(other_steps.len(), 1);
}

#[tokio::test]
async fn count_by_status_counts_correctly() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();

    let step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let mut step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());
    step2.status = TaskStepStatus::InProgress;
    let mut step3 = TaskStep::new(task_id.clone(), "Step 3".to_string(), 2, "user".to_string());
    step3.status = TaskStepStatus::Completed;
    let mut step4 = TaskStep::new(task_id.clone(), "Step 4".to_string(), 3, "user".to_string());
    step4.status = TaskStepStatus::Completed;

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();
    repo.create(step4).await.unwrap();

    let counts = repo.count_by_status(&task_id).await.unwrap();

    assert_eq!(counts.get(&TaskStepStatus::Pending), Some(&1));
    assert_eq!(counts.get(&TaskStepStatus::InProgress), Some(&1));
    assert_eq!(counts.get(&TaskStepStatus::Completed), Some(&2));
}

#[tokio::test]
async fn bulk_create_creates_all_steps() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();

    let steps = vec![
        TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string()),
        TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string()),
        TaskStep::new(task_id.clone(), "Step 3".to_string(), 2, "user".to_string()),
    ];

    let created = repo.bulk_create(steps).await.unwrap();
    assert_eq!(created.len(), 3);

    let retrieved = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(retrieved.len(), 3);
}

#[tokio::test]
async fn reorder_updates_sort_order() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();

    let step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());
    let step3 = TaskStep::new(task_id.clone(), "Step 3".to_string(), 2, "user".to_string());

    let id1 = step1.id.clone();
    let id2 = step2.id.clone();
    let id3 = step3.id.clone();

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();

    // Reorder: step3, step1, step2
    repo.reorder(&task_id, vec![id3.clone(), id1.clone(), id2.clone()])
        .await
        .unwrap();

    let steps = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(steps[0].id, id3);
    assert_eq!(steps[0].sort_order, 0);
    assert_eq!(steps[1].id, id1);
    assert_eq!(steps[1].sort_order, 1);
    assert_eq!(steps[2].id, id2);
    assert_eq!(steps[2].sort_order, 2);
}

#[tokio::test]
async fn reset_all_to_pending_resets_non_pending_steps() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();

    let step1 = TaskStep::new(
        task_id.clone(),
        "Pending".to_string(),
        0,
        "user".to_string(),
    );
    let mut step2 = TaskStep::new(
        task_id.clone(),
        "Completed".to_string(),
        1,
        "user".to_string(),
    );
    step2.status = TaskStepStatus::Completed;
    let mut step3 = TaskStep::new(
        task_id.clone(),
        "InProgress".to_string(),
        2,
        "user".to_string(),
    );
    step3.status = TaskStepStatus::InProgress;
    let mut step4 = TaskStep::new(task_id.clone(), "Failed".to_string(), 3, "user".to_string());
    step4.status = TaskStepStatus::Failed;
    let mut step5 = TaskStep::new(
        task_id.clone(),
        "Cancelled".to_string(),
        4,
        "user".to_string(),
    );
    step5.status = TaskStepStatus::Cancelled;

    repo.create(step1).await.unwrap();
    repo.create(step2).await.unwrap();
    repo.create(step3).await.unwrap();
    repo.create(step4).await.unwrap();
    repo.create(step5).await.unwrap();

    let count = repo.reset_all_to_pending(&task_id).await.unwrap();
    assert_eq!(count, 4, "Should reset 4 non-Pending steps");

    let steps = repo.get_by_task(&task_id).await.unwrap();
    for step in &steps {
        assert_eq!(step.status, TaskStepStatus::Pending);
        assert!(step.started_at.is_none());
        assert!(step.completed_at.is_none());
        assert!(step.completion_note.is_none());
    }
}

#[tokio::test]
async fn reset_all_to_pending_noop_when_all_pending() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();

    repo.create(TaskStep::new(
        task_id.clone(),
        "Step 1".to_string(),
        0,
        "user".to_string(),
    ))
    .await
    .unwrap();
    repo.create(TaskStep::new(
        task_id.clone(),
        "Step 2".to_string(),
        1,
        "user".to_string(),
    ))
    .await
    .unwrap();

    let count = repo.reset_all_to_pending(&task_id).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn reset_all_to_pending_only_affects_target_task() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();
    let other_task_id = TaskId::new();

    let mut step_target =
        TaskStep::new(task_id.clone(), "Target".to_string(), 0, "user".to_string());
    step_target.status = TaskStepStatus::Completed;
    let mut step_other = TaskStep::new(
        other_task_id.clone(),
        "Other".to_string(),
        0,
        "user".to_string(),
    );
    step_other.status = TaskStepStatus::Completed;
    let other_id = step_other.id.clone();

    repo.create(step_target).await.unwrap();
    repo.create(step_other).await.unwrap();

    let count = repo.reset_all_to_pending(&task_id).await.unwrap();
    assert_eq!(count, 1, "Only steps of target task should be reset");

    // Other task's step should remain Completed
    let other_step = repo.get_by_id(&other_id).await.unwrap().unwrap();
    assert_eq!(
        other_step.status,
        TaskStepStatus::Completed,
        "Other task's step should not be touched"
    );
}

#[tokio::test]
async fn reset_all_to_pending_preserves_structural_fields() {
    let repo = MemoryTaskStepRepository::new();
    let task_id = TaskId::new();

    let mut step = TaskStep::new(
        task_id.clone(),
        "Structural".to_string(),
        7,
        "user".to_string(),
    );
    step.status = TaskStepStatus::Skipped;
    step.description = Some("desc".to_string());
    step.scope_context = Some("scope".to_string());
    let step_id = step.id.clone();

    repo.create(step).await.unwrap();
    repo.reset_all_to_pending(&task_id).await.unwrap();

    let after = repo.get_by_id(&step_id).await.unwrap().unwrap();
    assert_eq!(after.title, "Structural");
    assert_eq!(after.sort_order, 7);
    assert_eq!(after.description, Some("desc".to_string()));
    assert_eq!(after.scope_context, Some("scope".to_string()));
    assert_eq!(after.status, TaskStepStatus::Pending);
}

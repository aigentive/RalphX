use super::*;

#[test]
fn task_step_new_creates_with_defaults() {
    let task_id = TaskId::new();
    let step = TaskStep::new(
        task_id.clone(),
        "Test step".to_string(),
        0,
        "user".to_string(),
    );

    assert_eq!(step.task_id, task_id);
    assert_eq!(step.title, "Test step");
    assert_eq!(step.sort_order, 0);
    assert_eq!(step.created_by, "user");
    assert_eq!(step.status, TaskStepStatus::Pending);
    assert!(step.description.is_none());
    assert!(step.depends_on.is_none());
    assert!(step.completion_note.is_none());
    assert!(step.started_at.is_none());
    assert!(step.completed_at.is_none());
}

#[test]
fn task_step_can_start_only_when_pending() {
    let task_id = TaskId::new();
    let mut step = TaskStep::new(task_id, "Test".to_string(), 0, "user".to_string());

    assert!(step.can_start());

    step.status = TaskStepStatus::InProgress;
    assert!(!step.can_start());

    step.status = TaskStepStatus::Completed;
    assert!(!step.can_start());
}

#[test]
fn task_step_is_terminal_for_final_states() {
    let task_id = TaskId::new();
    let mut step = TaskStep::new(task_id, "Test".to_string(), 0, "user".to_string());

    assert!(!step.is_terminal());

    step.status = TaskStepStatus::InProgress;
    assert!(!step.is_terminal());

    step.status = TaskStepStatus::Completed;
    assert!(step.is_terminal());

    step.status = TaskStepStatus::Skipped;
    assert!(step.is_terminal());

    step.status = TaskStepStatus::Failed;
    assert!(step.is_terminal());

    step.status = TaskStepStatus::Cancelled;
    assert!(step.is_terminal());
}

#[test]
fn task_step_status_to_db_string() {
    assert_eq!(TaskStepStatus::Pending.to_db_string(), "pending");
    assert_eq!(TaskStepStatus::InProgress.to_db_string(), "in_progress");
    assert_eq!(TaskStepStatus::Completed.to_db_string(), "completed");
    assert_eq!(TaskStepStatus::Skipped.to_db_string(), "skipped");
    assert_eq!(TaskStepStatus::Failed.to_db_string(), "failed");
    assert_eq!(TaskStepStatus::Cancelled.to_db_string(), "cancelled");
}

#[test]
fn task_step_status_from_db_string() {
    assert_eq!(
        TaskStepStatus::from_db_string("pending").unwrap(),
        TaskStepStatus::Pending
    );
    assert_eq!(
        TaskStepStatus::from_db_string("in_progress").unwrap(),
        TaskStepStatus::InProgress
    );
    assert_eq!(
        TaskStepStatus::from_db_string("completed").unwrap(),
        TaskStepStatus::Completed
    );
    assert_eq!(
        TaskStepStatus::from_db_string("skipped").unwrap(),
        TaskStepStatus::Skipped
    );
    assert_eq!(
        TaskStepStatus::from_db_string("failed").unwrap(),
        TaskStepStatus::Failed
    );
    assert_eq!(
        TaskStepStatus::from_db_string("cancelled").unwrap(),
        TaskStepStatus::Cancelled
    );
    assert!(TaskStepStatus::from_db_string("invalid").is_err());
}

#[test]
fn step_progress_summary_from_empty_steps() {
    let task_id = TaskId::new();
    let steps: Vec<TaskStep> = vec![];
    let summary = StepProgressSummary::from_steps(&task_id, &steps);

    assert_eq!(summary.total, 0);
    assert_eq!(summary.completed, 0);
    assert_eq!(summary.pending, 0);
    assert_eq!(summary.percent_complete, 0.0);
    assert!(summary.current_step.is_none());
    assert!(summary.next_step.is_none());
}

#[test]
fn step_progress_summary_calculates_correctly() {
    let task_id = TaskId::new();
    let mut steps = vec![
        TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string()),
        TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string()),
        TaskStep::new(task_id.clone(), "Step 3".to_string(), 2, "user".to_string()),
        TaskStep::new(task_id.clone(), "Step 4".to_string(), 3, "user".to_string()),
    ];

    steps[0].status = TaskStepStatus::Completed;
    steps[1].status = TaskStepStatus::InProgress;
    steps[2].status = TaskStepStatus::Pending;
    steps[3].status = TaskStepStatus::Skipped;

    let summary = StepProgressSummary::from_steps(&task_id, &steps);

    assert_eq!(summary.total, 4);
    assert_eq!(summary.completed, 1);
    assert_eq!(summary.in_progress, 1);
    assert_eq!(summary.pending, 1);
    assert_eq!(summary.skipped, 1);
    assert_eq!(summary.failed, 0);
    assert!((summary.percent_complete - 33.333332).abs() < 0.001);
    assert!(summary.current_step.is_some());
    assert_eq!(summary.current_step.unwrap().title, "Step 2");
    assert!(summary.next_step.is_some());
    assert_eq!(summary.next_step.unwrap().title, "Step 3");
}

#[test]
fn step_progress_summary_excludes_all_skipped_steps_from_percent() {
    let task_id = TaskId::new();
    let mut steps = vec![
        TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string()),
        TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string()),
    ];

    steps[0].status = TaskStepStatus::Skipped;
    steps[1].status = TaskStepStatus::Skipped;

    let summary = StepProgressSummary::from_steps(&task_id, &steps);

    assert_eq!(summary.total, 2);
    assert_eq!(summary.completed, 0);
    assert_eq!(summary.skipped, 2);
    assert_eq!(summary.percent_complete, 0.0);
}

#[test]
fn step_progress_summary_handles_all_completed() {
    let task_id = TaskId::new();
    let mut steps = vec![
        TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string()),
        TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string()),
    ];

    steps[0].status = TaskStepStatus::Completed;
    steps[1].status = TaskStepStatus::Completed;

    let summary = StepProgressSummary::from_steps(&task_id, &steps);

    assert_eq!(summary.total, 2);
    assert_eq!(summary.completed, 2);
    assert_eq!(summary.percent_complete, 100.0);
    assert!(summary.current_step.is_none());
    assert!(summary.next_step.is_none());
}

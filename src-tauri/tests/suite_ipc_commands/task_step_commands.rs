use ralphx_lib::application::AppState;
use ralphx_lib::commands::task_step_commands::{
    complete_step, create_task_step, delete_task_step, fail_step, get_step_progress,
    get_task_steps, reorder_task_steps, skip_step, start_step, update_task_step,
    CreateTaskStepInput, UpdateTaskStepInput,
};
use ralphx_lib::domain::entities::{
    Project, ProjectId, StepProgressSummary, TaskId, TaskStep, TaskStepStatus,
};
use tauri::Manager;

fn setup_test_state() -> AppState {
    AppState::new_test()
}

fn task_step_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

async fn create_test_project(state: &AppState) -> Project {
    let project = Project::new("Test Project".to_string(), "/tmp/test".to_string());
    state.project_repo.create(project.clone()).await.unwrap();
    project
}

async fn create_test_task(state: &AppState, project_id: ProjectId) -> TaskId {
    let task = ralphx_lib::domain::entities::Task::new(project_id, "Test Task".to_string());
    state.task_repo.create(task.clone()).await.unwrap();
    task.id
}

#[tokio::test]
async fn test_create_task_step() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Test repository directly
    let step = TaskStep::new(
        task_id.clone(),
        "Test Step".to_string(),
        0,
        "user".to_string(),
    );

    let created = state.task_step_repo.create(step).await.unwrap();

    assert_eq!(created.title, "Test Step");
    assert_eq!(created.sort_order, 0);
    assert_eq!(created.status, TaskStepStatus::Pending);
    assert_eq!(created.created_by, "user");
}

#[tokio::test]
async fn create_task_step_command_uses_defaults_and_optional_fields() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let response = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Command Step".to_string(),
            description: Some("Created by command".to_string()),
            sort_order: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("step creates");

    assert_eq!(response.task_id, task_id.as_str());
    assert_eq!(response.title, "Command Step");
    assert_eq!(response.description.as_deref(), Some("Created by command"));
    assert_eq!(response.status, "pending");
    assert_eq!(response.sort_order, 0);
    assert_eq!(response.created_by, "user");
}

#[tokio::test]
async fn test_get_task_steps() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create two steps
    let step1 = TaskStep::new(task_id.clone(), "Step 1".to_string(), 0, "user".to_string());
    let step2 = TaskStep::new(task_id.clone(), "Step 2".to_string(), 1, "user".to_string());

    state.task_step_repo.create(step1).await.unwrap();
    state.task_step_repo.create(step2).await.unwrap();

    let steps = state.task_step_repo.get_by_task(&task_id).await.unwrap();

    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].title, "Step 1");
    assert_eq!(steps[1].title, "Step 2");
}

#[tokio::test]
async fn get_task_steps_command_returns_ordered_responses() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "First".to_string(),
            description: None,
            sort_order: Some(0),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("first step creates");
    create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Second".to_string(),
            description: None,
            sort_order: Some(1),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("second step creates");

    let response = get_task_steps(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("steps list");

    assert_eq!(response.len(), 2);
    assert_eq!(response[0].title, "First");
    assert_eq!(response[1].title, "Second");
}

#[tokio::test]
async fn test_update_task_step() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    let step = TaskStep::new(
        task_id.clone(),
        "Original Title".to_string(),
        0,
        "user".to_string(),
    );

    let created = state.task_step_repo.create(step).await.unwrap();

    let mut updated = created.clone();
    updated.title = "Updated Title".to_string();
    updated.description = Some("Updated Description".to_string());

    state.task_step_repo.update(&updated).await.unwrap();

    let found = state
        .task_step_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.title, "Updated Title");
    assert_eq!(found.description, Some("Updated Description".to_string()));
    assert_eq!(found.sort_order, 0); // Unchanged
}

#[tokio::test]
async fn update_task_step_command_applies_partial_fields_and_rejects_missing_step() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let missing_error = update_task_step(
        "missing-step".to_string(),
        UpdateTaskStepInput {
            title: Some("Nope".to_string()),
            description: None,
            sort_order: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("missing step should error");
    assert!(missing_error
        .to_string()
        .contains("Step missing-step not found"));

    let created = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Original".to_string(),
            description: None,
            sort_order: Some(0),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("step creates");

    let response = update_task_step(
        created.id.clone(),
        UpdateTaskStepInput {
            title: Some("Updated".to_string()),
            description: Some("Updated description".to_string()),
            sort_order: Some(7),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("step updates");

    assert_eq!(response.title, "Updated");
    assert_eq!(response.description.as_deref(), Some("Updated description"));
    assert_eq!(response.sort_order, 7);
    assert_eq!(response.status, "pending");
}

#[tokio::test]
async fn test_reorder_task_steps() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create three steps
    let step1 = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Step 1".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    let step2 = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Step 2".to_string(),
            1,
            "user".to_string(),
        ))
        .await
        .unwrap();

    let step3 = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Step 3".to_string(),
            2,
            "user".to_string(),
        ))
        .await
        .unwrap();

    // Reorder: 3, 1, 2
    let new_order = vec![step3.id.clone(), step1.id.clone(), step2.id.clone()];
    state
        .task_step_repo
        .reorder(&task_id, new_order)
        .await
        .unwrap();

    let reordered = state.task_step_repo.get_by_task(&task_id).await.unwrap();

    assert_eq!(reordered.len(), 3);
    assert_eq!(reordered[0].title, "Step 3");
    assert_eq!(reordered[0].sort_order, 0);
    assert_eq!(reordered[1].title, "Step 1");
    assert_eq!(reordered[1].sort_order, 1);
    assert_eq!(reordered[2].title, "Step 2");
    assert_eq!(reordered[2].sort_order, 2);
}

#[tokio::test]
async fn reorder_task_steps_command_reorders_and_returns_updated_steps() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let first = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "First".to_string(),
            description: None,
            sort_order: Some(0),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("first creates");
    let second = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Second".to_string(),
            description: None,
            sort_order: Some(1),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("second creates");
    let third = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Third".to_string(),
            description: None,
            sort_order: Some(2),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("third creates");

    let reordered = reorder_task_steps(
        task_id.as_str().to_string(),
        vec![third.id.clone(), first.id.clone(), second.id.clone()],
        app.state::<AppState>(),
    )
    .await
    .expect("steps reorder");

    assert_eq!(reordered.len(), 3);
    assert_eq!(reordered[0].title, "Third");
    assert_eq!(reordered[0].sort_order, 0);
    assert_eq!(reordered[1].title, "First");
    assert_eq!(reordered[1].sort_order, 1);
    assert_eq!(reordered[2].title, "Second");
    assert_eq!(reordered[2].sort_order, 2);
}

#[tokio::test]
async fn test_get_step_progress() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create steps with different statuses
    let step1 = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Step 1".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Step 2".to_string(),
            1,
            "user".to_string(),
        ))
        .await
        .unwrap();

    // Mark step 1 as completed
    let mut step1_entity = state
        .task_step_repo
        .get_by_id(&step1.id)
        .await
        .unwrap()
        .unwrap();
    step1_entity.status = TaskStepStatus::Completed;
    state.task_step_repo.update(&step1_entity).await.unwrap();

    let steps = state.task_step_repo.get_by_task(&task_id).await.unwrap();
    let progress = StepProgressSummary::from_steps(&task_id, &steps);

    assert_eq!(progress.total, 2);
    assert_eq!(progress.completed, 1);
    assert_eq!(progress.pending, 1);
    assert_eq!(progress.percent_complete, 50.0);
}

#[tokio::test]
async fn get_step_progress_ipc_contract_excludes_skipped_steps_from_percent() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let mut steps = Vec::new();
    for (sort_order, title) in ["Done", "Active", "Pending", "Skipped A", "Skipped B"]
        .into_iter()
        .enumerate()
    {
        steps.push(
            create_task_step(
                task_id.as_str().to_string(),
                CreateTaskStepInput {
                    title: title.to_string(),
                    description: None,
                    sort_order: Some(sort_order as i32),
                },
                app.state::<AppState>(),
            )
            .await
            .expect("step creates"),
        );
    }

    start_step(steps[0].id.clone(), app.state::<AppState>())
        .await
        .expect("first step starts");
    complete_step(
        steps[0].id.clone(),
        Some("done".to_string()),
        app.state::<AppState>(),
    )
    .await
    .expect("first step completes");
    start_step(steps[1].id.clone(), app.state::<AppState>())
        .await
        .expect("second step starts");
    skip_step(
        steps[3].id.clone(),
        "not needed".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("fourth step skips");
    skip_step(
        steps[4].id.clone(),
        "not needed".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("fifth step skips");

    let progress = get_step_progress(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("progress loads");

    assert_eq!(progress.total, 5);
    assert_eq!(progress.completed, 1);
    assert_eq!(progress.in_progress, 1);
    assert_eq!(progress.pending, 1);
    assert_eq!(progress.skipped, 2);
    assert!((progress.percent_complete - 33.333332).abs() < 0.001);
}

#[tokio::test]
async fn get_step_progress_command_summarizes_current_and_next_steps() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let current = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Current".to_string(),
            description: None,
            sort_order: Some(0),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("current creates");
    let next = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Next".to_string(),
            description: None,
            sort_order: Some(1),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("next creates");

    start_step(current.id.clone(), app.state::<AppState>())
        .await
        .expect("current starts");

    let progress = get_step_progress(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("progress loads");

    assert_eq!(progress.total, 2);
    assert_eq!(progress.in_progress, 1);
    assert_eq!(progress.pending, 1);
    assert_eq!(
        progress.current_step.expect("current step").id.as_str(),
        current.id
    );
    assert_eq!(progress.next_step.expect("next step").id.as_str(), next.id);
}

#[tokio::test]
async fn test_start_step_valid() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create a pending step
    let step = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Test Step".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    // Start the step via command (simulating tauri command)
    let mut updated = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    updated.status = TaskStepStatus::InProgress;
    updated.started_at = Some(chrono::Utc::now());
    updated.touch();
    state.task_step_repo.update(&updated).await.unwrap();

    // Verify status changed
    let found = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.status, TaskStepStatus::InProgress);
    assert!(found.started_at.is_some());
}

#[tokio::test]
async fn start_step_command_sets_in_progress_and_rejects_invalid_status() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let missing_error = start_step("missing-step".to_string(), app.state::<AppState>())
        .await
        .expect_err("missing step should error");
    assert!(missing_error
        .to_string()
        .contains("Step missing-step not found"));

    let step = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Start me".to_string(),
            description: None,
            sort_order: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("step creates");

    let started = start_step(step.id.clone(), app.state::<AppState>())
        .await
        .expect("step starts");
    assert_eq!(started.status, "in_progress");
    assert!(started.started_at.is_some());

    let second_start = start_step(step.id, app.state::<AppState>())
        .await
        .expect_err("in-progress step cannot start again");
    assert!(second_start
        .to_string()
        .contains("Step must be Pending to start"));
}

#[tokio::test]
async fn test_start_step_invalid_status() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create a step and mark it as completed
    let mut step = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Test Step".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    step.status = TaskStepStatus::Completed;
    state.task_step_repo.update(&step).await.unwrap();

    // Trying to start a completed step should fail
    // In actual command this would return AppError::Validation
    let found = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.status, TaskStepStatus::Completed);
    assert_ne!(found.status, TaskStepStatus::Pending);
}

#[tokio::test]
async fn test_complete_step_valid() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create and start a step
    let mut step = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Test Step".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    step.status = TaskStepStatus::InProgress;
    step.started_at = Some(chrono::Utc::now());
    state.task_step_repo.update(&step).await.unwrap();

    // Complete the step
    let mut found = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    found.status = TaskStepStatus::Completed;
    found.completed_at = Some(chrono::Utc::now());
    found.completion_note = Some("Done!".to_string());
    found.touch();
    state.task_step_repo.update(&found).await.unwrap();

    // Verify
    let completed = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.status, TaskStepStatus::Completed);
    assert!(completed.completed_at.is_some());
    assert_eq!(completed.completion_note, Some("Done!".to_string()));
}

#[tokio::test]
async fn complete_step_command_requires_in_progress_and_records_note() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let step = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Complete me".to_string(),
            description: None,
            sort_order: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("step creates");

    let premature = complete_step(
        step.id.clone(),
        Some("too soon".to_string()),
        app.state::<AppState>(),
    )
    .await
    .expect_err("pending step cannot complete");
    assert!(premature
        .to_string()
        .contains("Step must be InProgress to complete"));

    start_step(step.id.clone(), app.state::<AppState>())
        .await
        .expect("step starts");
    let completed = complete_step(
        step.id.clone(),
        Some("Finished cleanly".to_string()),
        app.state::<AppState>(),
    )
    .await
    .expect("step completes");

    assert_eq!(completed.status, "completed");
    assert_eq!(
        completed.completion_note.as_deref(),
        Some("Finished cleanly")
    );
    assert!(completed.completed_at.is_some());
}

#[tokio::test]
async fn test_complete_step_invalid_status() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create a pending step (not in progress)
    let step = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Test Step".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    // Trying to complete a pending step should fail in actual command
    let found = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.status, TaskStepStatus::Pending);
    assert_ne!(found.status, TaskStepStatus::InProgress);
}

#[tokio::test]
async fn test_skip_step_from_pending() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create a pending step
    let step = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Test Step".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    // Skip the step
    let mut found = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    found.status = TaskStepStatus::Skipped;
    found.completed_at = Some(chrono::Utc::now());
    found.completion_note = Some("Not needed".to_string());
    found.touch();
    state.task_step_repo.update(&found).await.unwrap();

    // Verify
    let skipped = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(skipped.status, TaskStepStatus::Skipped);
    assert!(skipped.completed_at.is_some());
    assert_eq!(skipped.completion_note, Some("Not needed".to_string()));
}

#[tokio::test]
async fn skip_step_command_accepts_pending_and_rejects_terminal_status() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let step = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Skip me".to_string(),
            description: None,
            sort_order: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("step creates");

    let skipped = skip_step(
        step.id.clone(),
        "No longer needed".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("pending step skips");
    assert_eq!(skipped.status, "skipped");
    assert_eq!(skipped.completion_note.as_deref(), Some("No longer needed"));

    let skip_again = skip_step(step.id, "still no".to_string(), app.state::<AppState>())
        .await
        .expect_err("terminal skipped step cannot skip again");
    assert!(skip_again
        .to_string()
        .contains("Step must be Pending or InProgress to skip"));
}

#[tokio::test]
async fn test_skip_step_from_in_progress() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create and start a step
    let mut step = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Test Step".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    step.status = TaskStepStatus::InProgress;
    step.started_at = Some(chrono::Utc::now());
    state.task_step_repo.update(&step).await.unwrap();

    // Skip the step
    let mut found = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    found.status = TaskStepStatus::Skipped;
    found.completed_at = Some(chrono::Utc::now());
    found.completion_note = Some("Changed approach".to_string());
    found.touch();
    state.task_step_repo.update(&found).await.unwrap();

    // Verify
    let skipped = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(skipped.status, TaskStepStatus::Skipped);
    assert!(skipped.completed_at.is_some());
}

#[tokio::test]
async fn test_fail_step_valid() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create and start a step
    let mut step = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Test Step".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    step.status = TaskStepStatus::InProgress;
    step.started_at = Some(chrono::Utc::now());
    state.task_step_repo.update(&step).await.unwrap();

    // Fail the step
    let mut found = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    found.status = TaskStepStatus::Failed;
    found.completed_at = Some(chrono::Utc::now());
    found.completion_note = Some("Build error".to_string());
    found.touch();
    state.task_step_repo.update(&found).await.unwrap();

    // Verify
    let failed = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.status, TaskStepStatus::Failed);
    assert!(failed.completed_at.is_some());
    assert_eq!(failed.completion_note, Some("Build error".to_string()));
}

#[tokio::test]
async fn fail_step_command_requires_in_progress_and_records_error() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let step = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Fail me".to_string(),
            description: None,
            sort_order: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("step creates");

    let premature = fail_step(
        step.id.clone(),
        "not yet".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("pending step cannot fail");
    assert!(premature
        .to_string()
        .contains("Step must be InProgress to fail"));

    start_step(step.id.clone(), app.state::<AppState>())
        .await
        .expect("step starts");
    let failed = fail_step(
        step.id.clone(),
        "Build failed".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("step fails");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.completion_note.as_deref(), Some("Build failed"));
    assert!(failed.completed_at.is_some());
}

#[tokio::test]
async fn delete_task_step_command_removes_step() {
    let app = task_step_command_app();
    let project = create_test_project(app.state::<AppState>().inner()).await;
    let task_id = create_test_task(app.state::<AppState>().inner(), project.id).await;

    let step = create_task_step(
        task_id.as_str().to_string(),
        CreateTaskStepInput {
            title: "Delete me".to_string(),
            description: None,
            sort_order: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("step creates");

    delete_task_step(step.id, app.state::<AppState>())
        .await
        .expect("step deletes");

    let steps = get_task_steps(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("steps list after delete");
    assert!(steps.is_empty());
}

#[tokio::test]
async fn test_fail_step_invalid_status() {
    let state = setup_test_state();
    let project = create_test_project(&state).await;
    let task_id = create_test_task(&state, project.id).await;

    // Create a pending step (not in progress)
    let step = state
        .task_step_repo
        .create(TaskStep::new(
            task_id.clone(),
            "Test Step".to_string(),
            0,
            "user".to_string(),
        ))
        .await
        .unwrap();

    // Trying to fail a pending step should fail in actual command
    let found = state
        .task_step_repo
        .get_by_id(&step.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.status, TaskStepStatus::Pending);
    assert_ne!(found.status, TaskStepStatus::InProgress);
}

// ── IPC contract tests ─────────────────────────────────────────────────────────
// Verify camelCase deserialization for task step command input structs.

#[cfg(test)]
mod ipc_contract {
    use ralphx_lib::commands::task_step_commands::{CreateTaskStepInput, UpdateTaskStepInput};

    // ── CreateTaskStepInput ─────────────────────────────────────────────────

    #[test]
    fn create_task_step_input_deserializes_camel_case() {
        let json = r#"{"title":"Implement auth","description":"Add JWT middleware","sortOrder":0}"#;
        let input: CreateTaskStepInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.title, "Implement auth");
        assert_eq!(input.description, Some("Add JWT middleware".to_string()));
        assert_eq!(input.sort_order, Some(0));
    }

    #[test]
    fn create_task_step_input_optional_fields_absent() {
        let json = r#"{"title":"Minimal step"}"#;
        let input: CreateTaskStepInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.title, "Minimal step");
        assert!(input.description.is_none());
        assert!(input.sort_order.is_none());
    }

    #[test]
    fn create_task_step_input_snake_case_sort_order_ignored() {
        // sort_order in snake_case must not map to sortOrder field due to rename_all
        let json = r#"{"title":"Bad","sort_order":5}"#;
        let input: CreateTaskStepInput = serde_json::from_str(json).unwrap();
        assert!(
            input.sort_order.is_none(),
            "snake_case sort_order must not deserialize into sortOrder field"
        );
    }

    // ── UpdateTaskStepInput ─────────────────────────────────────────────────

    #[test]
    fn update_task_step_input_deserializes_all_camel_case_fields() {
        let json = r#"{"title":"Updated title","description":"New desc","sortOrder":3}"#;
        let input: UpdateTaskStepInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.title, Some("Updated title".to_string()));
        assert_eq!(input.description, Some("New desc".to_string()));
        assert_eq!(input.sort_order, Some(3));
    }

    #[test]
    fn update_task_step_input_partial_update() {
        let json = r#"{"title":"Only title changed"}"#;
        let input: UpdateTaskStepInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.title, Some("Only title changed".to_string()));
        assert!(input.description.is_none());
        assert!(input.sort_order.is_none());
    }

    #[test]
    fn update_task_step_input_empty_object() {
        let json = r#"{}"#;
        let input: UpdateTaskStepInput = serde_json::from_str(json).unwrap();
        assert!(input.title.is_none());
        assert!(input.description.is_none());
        assert!(input.sort_order.is_none());
    }
}

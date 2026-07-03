use ralphx_lib::application::AppState;
use ralphx_lib::commands::qa_commands::{
    get_qa_results, get_qa_settings, get_task_qa, retry_qa, skip_qa, update_qa_settings,
    QAResultsResponse, TaskQAResponse, UpdateQASettingsInput,
};
use ralphx_lib::domain::entities::{TaskId, TaskQA};
use ralphx_lib::domain::qa::{
    AcceptanceCriteria, AcceptanceCriterion, QAOverallStatus, QAResults, QAStepResult,
    QAStepStatus, QATestStep, QATestSteps,
};
use tauri::Manager;

async fn setup_test_state() -> AppState {
    AppState::new_test()
}

fn qa_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

// ==================== QA Settings Tests ====================

#[tokio::test]
async fn test_get_qa_settings_returns_default() {
    let state = setup_test_state().await;

    let settings = state.qa_settings.read().await;

    assert!(settings.qa_enabled);
    assert!(settings.auto_qa_for_ui_tasks);
    assert!(!settings.auto_qa_for_api_tasks);
    assert_eq!(settings.browser_testing_url, "http://localhost:1420");
}

#[tokio::test]
async fn test_update_qa_settings_partial_update() {
    let state = setup_test_state().await;

    // Update only some fields
    {
        let mut settings = state.qa_settings.write().await;
        settings.qa_enabled = false;
        settings.browser_testing_url = "http://localhost:3000".to_string();
    }

    let settings = state.qa_settings.read().await;
    assert!(!settings.qa_enabled);
    assert!(settings.auto_qa_for_ui_tasks); // Unchanged
    assert_eq!(settings.browser_testing_url, "http://localhost:3000");
}

#[tokio::test]
async fn update_qa_settings_command_only_changes_provided_fields() {
    let app = qa_command_app();

    let defaults = get_qa_settings(app.state::<AppState>())
        .await
        .expect("default QA settings load");
    assert!(defaults.qa_enabled);
    assert!(defaults.auto_qa_for_ui_tasks);

    let updated = update_qa_settings(
        UpdateQASettingsInput {
            qa_enabled: Some(false),
            auto_qa_for_ui_tasks: None,
            auto_qa_for_api_tasks: Some(true),
            qa_prep_enabled: None,
            browser_testing_enabled: Some(false),
            browser_testing_url: Some("http://localhost:5173".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("QA settings update");

    assert!(!updated.qa_enabled);
    assert!(updated.auto_qa_for_ui_tasks);
    assert!(updated.auto_qa_for_api_tasks);
    assert_eq!(updated.qa_prep_enabled, defaults.qa_prep_enabled);
    assert!(!updated.browser_testing_enabled);
    assert_eq!(updated.browser_testing_url, "http://localhost:5173");
}

// ==================== TaskQA Tests ====================

#[tokio::test]
async fn test_get_task_qa_returns_none_for_missing() {
    let state = setup_test_state().await;
    let task_id = TaskId::from_string("nonexistent".to_string());

    let result = state.task_qa_repo.get_by_task_id(&task_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_task_qa_returns_existing() {
    let state = setup_test_state().await;
    let task_id = TaskId::from_string("task-123".to_string());

    let task_qa = TaskQA::new(task_id.clone());
    state.task_qa_repo.create(&task_qa).await.unwrap();

    let result = state.task_qa_repo.get_by_task_id(&task_id).await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().task_id, task_id);
}

#[tokio::test]
async fn task_qa_commands_return_optional_responses() {
    let app = qa_command_app();
    let task_id = TaskId::from_string("task-command-qa".to_string());

    let missing = get_task_qa(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("missing task QA should not error");
    assert!(missing.is_none());

    let mut task_qa = TaskQA::new(task_id.clone());
    task_qa.acceptance_criteria = Some(AcceptanceCriteria::from_criteria(vec![
        AcceptanceCriterion::behavior("AC1", "Behavior works"),
    ]));
    app.state::<AppState>()
        .task_qa_repo
        .create(&task_qa)
        .await
        .expect("TaskQA creates");

    let response = get_task_qa(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("TaskQA should load")
        .expect("TaskQA response should exist");
    assert_eq!(response.task_id, task_id.as_str());
    assert_eq!(response.acceptance_criteria.expect("criteria").len(), 1);
}

// ==================== QA Results Tests ====================

#[tokio::test]
async fn test_get_qa_results_returns_none_for_missing_task() {
    let state = setup_test_state().await;
    let task_id = TaskId::from_string("nonexistent".to_string());

    let result = state.task_qa_repo.get_by_task_id(&task_id).await.unwrap();
    let results = result.and_then(|qa| qa.test_results);
    assert!(results.is_none());
}

#[tokio::test]
async fn test_get_qa_results_returns_none_when_no_results() {
    let state = setup_test_state().await;
    let task_id = TaskId::from_string("task-123".to_string());

    let task_qa = TaskQA::new(task_id.clone());
    state.task_qa_repo.create(&task_qa).await.unwrap();

    let result = state.task_qa_repo.get_by_task_id(&task_id).await.unwrap();
    let results = result.and_then(|qa| qa.test_results);
    assert!(results.is_none());
}

#[tokio::test]
async fn test_get_qa_results_returns_results() {
    let state = setup_test_state().await;
    let task_id = TaskId::from_string("task-123".to_string());

    let task_qa = TaskQA::new(task_id.clone());
    let qa_id = task_qa.id.clone();
    state.task_qa_repo.create(&task_qa).await.unwrap();

    // Add results
    let results =
        QAResults::from_results(task_id.as_str(), vec![QAStepResult::passed("QA1", None)]);
    state
        .task_qa_repo
        .update_results(&qa_id, "agent-1", &results, &[])
        .await
        .unwrap();

    let result = state.task_qa_repo.get_by_task_id(&task_id).await.unwrap();
    let qa_results = result.and_then(|qa| qa.test_results);
    assert!(qa_results.is_some());
    assert!(qa_results.unwrap().is_passed());
}

#[tokio::test]
async fn get_qa_results_command_returns_only_results_payload() {
    let app = qa_command_app();
    let task_id = TaskId::from_string("task-command-results".to_string());
    let task_qa = TaskQA::new(task_id.clone());
    let qa_id = task_qa.id.clone();
    app.state::<AppState>()
        .task_qa_repo
        .create(&task_qa)
        .await
        .expect("TaskQA creates");

    let empty = get_qa_results(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("missing results should not error");
    assert!(empty.is_none());

    let results = QAResults::from_results(
        task_id.as_str(),
        vec![QAStepResult::failed_comparison(
            "QA1",
            "expected copy",
            "actual copy",
            Some("failure.png".to_string()),
        )],
    );
    app.state::<AppState>()
        .task_qa_repo
        .update_results(&qa_id, "agent-1", &results, &["failure.png".to_string()])
        .await
        .expect("results update");

    let response = get_qa_results(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("results should load")
        .expect("results response should exist");
    assert_eq!(response.overall_status, "failed");
    assert_eq!(response.steps[0].expected.as_deref(), Some("expected copy"));
    assert_eq!(response.steps[0].actual.as_deref(), Some("actual copy"));
    assert_eq!(response.steps[0].screenshot.as_deref(), Some("failure.png"));
}

// ==================== Retry QA Tests ====================

#[tokio::test]
async fn test_retry_qa_resets_results() {
    let state = setup_test_state().await;
    let task_id = TaskId::from_string("task-123".to_string());

    // Create TaskQA with test steps
    let mut task_qa = TaskQA::new(task_id.clone());
    let qa_id = task_qa.id.clone();

    // Add test steps (needed for retry to generate step IDs)
    let steps = QATestSteps::from_steps(vec![QATestStep::new(
        "QA1",
        "AC1",
        "Test step",
        vec![],
        "Expected",
    )]);
    task_qa.qa_test_steps = Some(steps);
    state.task_qa_repo.create(&task_qa).await.unwrap();

    // Add failed results
    let failed_results = QAResults::from_results(
        task_id.as_str(),
        vec![QAStepResult::failed("QA1", "Something went wrong", None)],
    );
    state
        .task_qa_repo
        .update_results(&qa_id, "agent-1", &failed_results, &[])
        .await
        .unwrap();

    // Verify failed
    let before = state
        .task_qa_repo
        .get_by_task_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert!(before.is_failed());

    // Retry
    let step_ids = before
        .effective_test_steps()
        .map(|s| s.qa_steps.iter().map(|step| step.id.clone()).collect())
        .unwrap_or_default();
    let fresh_results = QAResults::new(task_id.as_str(), step_ids);
    state
        .task_qa_repo
        .update_results(&qa_id, "", &fresh_results, &[])
        .await
        .unwrap();

    // Verify reset
    let after = state
        .task_qa_repo
        .get_by_task_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let results = after.test_results.unwrap();
    assert_eq!(results.overall_status, QAOverallStatus::Pending);
}

#[tokio::test]
async fn retry_qa_command_resets_effective_refined_steps_and_errors_when_missing() {
    let app = qa_command_app();

    let missing_error = retry_qa("missing-task".to_string(), app.state::<AppState>())
        .await
        .expect_err("missing TaskQA should error");
    assert!(missing_error.contains("No QA record found for task: missing-task"));

    let task_id = TaskId::from_string("task-command-retry".to_string());
    let mut task_qa = TaskQA::new(task_id.clone());
    task_qa.qa_test_steps = Some(QATestSteps::from_steps(vec![QATestStep::new(
        "QA-original",
        "AC1",
        "Original step",
        vec![],
        "Expected",
    )]));
    task_qa.complete_refinement(
        "agent-refine".to_string(),
        "Implementation summary".to_string(),
        QATestSteps::from_steps(vec![QATestStep::new(
            "QA-refined",
            "AC1",
            "Refined step",
            vec!["click".to_string()],
            "Expected refined",
        )]),
    );
    let qa_id = task_qa.id.clone();
    app.state::<AppState>()
        .task_qa_repo
        .create(&task_qa)
        .await
        .expect("TaskQA creates");
    app.state::<AppState>()
        .task_qa_repo
        .update_results(
            &qa_id,
            "agent-1",
            &QAResults::from_results(
                task_id.as_str(),
                vec![QAStepResult::failed("QA-refined", "failed", None)],
            ),
            &[],
        )
        .await
        .expect("failed results save");

    let response = retry_qa(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("retry should reset results");
    let results = response.test_results.expect("retry writes pending results");
    assert_eq!(results.overall_status, "pending");
    assert_eq!(results.steps.len(), 1);
    assert_eq!(results.steps[0].step_id, "QA-refined");
    assert_eq!(results.steps[0].status, "pending");
}

// ==================== Skip QA Tests ====================

#[tokio::test]
async fn test_skip_qa_marks_as_skipped() {
    let state = setup_test_state().await;
    let task_id = TaskId::from_string("task-123".to_string());

    // Create TaskQA with test steps
    let mut task_qa = TaskQA::new(task_id.clone());
    let qa_id = task_qa.id.clone();

    let steps = QATestSteps::from_steps(vec![QATestStep::new(
        "QA1",
        "AC1",
        "Test step",
        vec![],
        "Expected",
    )]);
    task_qa.qa_test_steps = Some(steps);
    state.task_qa_repo.create(&task_qa).await.unwrap();

    // Skip QA
    let step_ids: Vec<String> = vec!["QA1".to_string()];
    let skipped_results = QAResults::from_results(
        task_id.as_str(),
        step_ids
            .into_iter()
            .map(|id| QAStepResult::skipped(id, Some("QA skipped by user".to_string())))
            .collect(),
    );
    state
        .task_qa_repo
        .update_results(&qa_id, "user-skip", &skipped_results, &[])
        .await
        .unwrap();

    // Verify skipped (which counts as not passed/failed but complete)
    let after = state
        .task_qa_repo
        .get_by_task_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let results = after.test_results.unwrap();
    assert_eq!(results.steps[0].status, QAStepStatus::Skipped);
}

#[tokio::test]
async fn skip_qa_command_marks_all_effective_steps_skipped_and_errors_when_missing() {
    let app = qa_command_app();

    let missing_error = skip_qa("missing-skip-task".to_string(), app.state::<AppState>())
        .await
        .expect_err("missing TaskQA should error");
    assert!(missing_error.contains("No QA record found for task: missing-skip-task"));

    let task_id = TaskId::from_string("task-command-skip".to_string());
    let mut task_qa = TaskQA::new(task_id.clone());
    task_qa.qa_test_steps = Some(QATestSteps::from_steps(vec![
        QATestStep::new("QA1", "AC1", "Step one", vec![], "Expected one"),
        QATestStep::new("QA2", "AC2", "Step two", vec![], "Expected two"),
    ]));
    app.state::<AppState>()
        .task_qa_repo
        .create(&task_qa)
        .await
        .expect("TaskQA creates");

    let response = skip_qa(task_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("skip should write skipped results");
    let results = response.test_results.expect("skip writes results");
    assert_eq!(results.overall_status, "pending");
    assert_eq!(results.steps.len(), 2);
    assert!(results.steps.iter().all(|step| step.status == "skipped"));
    assert!(
        results
            .steps
            .iter()
            .all(|step| step.error.as_deref() == Some("QA skipped by user"))
    );
}

// ==================== Response Conversion Tests ====================

#[tokio::test]
async fn test_task_qa_response_conversion() {
    let task_id = TaskId::from_string("task-123".to_string());
    let mut task_qa = TaskQA::new(task_id.clone());

    // Add acceptance criteria
    let criteria =
        AcceptanceCriteria::from_criteria(vec![AcceptanceCriterion::visual("AC1", "Visual test")]);
    let steps = QATestSteps::from_steps(vec![QATestStep::new(
        "QA1",
        "AC1",
        "Test step",
        vec!["cmd1".to_string()],
        "Expected",
    )]);

    task_qa.acceptance_criteria = Some(criteria);
    task_qa.qa_test_steps = Some(steps);

    let response = TaskQAResponse::from(task_qa);

    assert_eq!(response.task_id, "task-123");
    assert!(response.acceptance_criteria.is_some());
    assert_eq!(response.acceptance_criteria.unwrap().len(), 1);
    assert!(response.qa_test_steps.is_some());
    assert_eq!(response.qa_test_steps.unwrap().len(), 1);
}

#[tokio::test]
async fn task_qa_response_conversion_includes_refinement_results_and_timestamps() {
    let task_id = TaskId::from_string("task-response-full".to_string());
    let mut task_qa = TaskQA::new(task_id.clone());
    task_qa.start_prep("prep-agent".to_string());
    task_qa.complete_prep(
        AcceptanceCriteria::from_criteria(vec![AcceptanceCriterion::visual("AC1", "Visual")]),
        QATestSteps::from_steps(vec![QATestStep::new(
            "QA1",
            "AC1",
            "Initial step",
            vec![],
            "Expected",
        )]),
    );
    task_qa.complete_refinement(
        "refine-agent".to_string(),
        "Implemented button behavior".to_string(),
        QATestSteps::from_steps(vec![QATestStep::new(
            "QA-refined",
            "AC1",
            "Refined step",
            vec!["click button".to_string()],
            "Button responds",
        )]),
    );
    task_qa.start_testing("test-agent".to_string());
    task_qa.complete_testing(QAResults::from_results(
        task_id.as_str(),
        vec![QAStepResult::passed(
            "QA-refined",
            Some("passed.png".to_string()),
        )],
    ));
    task_qa.screenshots = vec!["passed.png".to_string()];

    let response = TaskQAResponse::from(task_qa);

    assert_eq!(response.prep_agent_id.as_deref(), Some("prep-agent"));
    assert!(response.prep_started_at.is_some());
    assert!(response.prep_completed_at.is_some());
    assert_eq!(
        response.actual_implementation.as_deref(),
        Some("Implemented button behavior")
    );
    assert_eq!(response.refinement_agent_id.as_deref(), Some("refine-agent"));
    assert!(response.refinement_completed_at.is_some());
    assert_eq!(response.refined_test_steps.expect("refined steps").len(), 1);
    assert_eq!(response.test_agent_id.as_deref(), Some("test-agent"));
    assert!(response.test_completed_at.is_some());
    assert_eq!(response.screenshots, vec!["passed.png"]);
    assert_eq!(
        response
            .test_results
            .expect("test results")
            .steps[0]
            .screenshot
            .as_deref(),
        Some("passed.png")
    );
}

#[tokio::test]
async fn test_qa_results_response_conversion() {
    let results = QAResults::from_results(
        "task-123",
        vec![
            QAStepResult::passed("QA1", Some("ss1.png".to_string())),
            QAStepResult::failed("QA2", "Error", None),
        ],
    );

    let response = QAResultsResponse::from(results);

    assert_eq!(response.task_id, "task-123");
    assert_eq!(response.overall_status, "failed");
    assert_eq!(response.total_steps, 2);
    assert_eq!(response.passed_steps, 1);
    assert_eq!(response.failed_steps, 1);
    assert_eq!(response.steps.len(), 2);
    assert_eq!(response.steps[0].status, "passed");
    assert_eq!(response.steps[1].status, "failed");
}

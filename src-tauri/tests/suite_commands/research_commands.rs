use ralphx_lib::application::AppState;
use ralphx_lib::commands::research_commands::{
    get_research_presets, get_research_process, get_research_processes, pause_research,
    resume_research, start_research, stop_research, CustomDepthInput, ResearchProcessResponse,
    StartResearchInput,
};
use ralphx_lib::domain::entities::{
    ResearchBrief, ResearchDepthPreset, ResearchProcess, ResearchProcessStatus,
};
use tauri::Manager;

fn setup_test_state() -> AppState {
    AppState::new_test()
}

fn research_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

fn create_test_process() -> ResearchProcess {
    let brief = ResearchBrief::new("What architecture should we use?");
    ResearchProcess::new("Test Research", brief, "deep-researcher")
        .with_preset(ResearchDepthPreset::Standard)
}

#[tokio::test]
async fn test_create_research_process() {
    let state = setup_test_state();

    let process = create_test_process();
    let created = state.process_repo.create(process).await.unwrap();

    assert_eq!(created.name, "Test Research");
    assert_eq!(created.agent_profile_id, "deep-researcher");
}

#[tokio::test]
async fn start_research_command_applies_custom_depth_brief_and_output() {
    let app = research_command_app();

    let response = start_research(
        StartResearchInput {
            name: "Command Research".to_string(),
            question: "How should commands be tested?".to_string(),
            context: Some("Backend command coverage".to_string()),
            scope: Some("src-tauri commands".to_string()),
            constraints: Some(vec!["test-only".to_string(), "no network".to_string()]),
            agent_profile_id: "researcher".to_string(),
            depth_preset: Some("standard".to_string()),
            custom_depth: Some(CustomDepthInput {
                max_iterations: 7,
                timeout_hours: 1.5,
                checkpoint_interval: 2,
            }),
            target_bucket: Some("command-coverage".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("research should start");

    assert_eq!(response.name, "Command Research");
    assert_eq!(response.question, "How should commands be tested?");
    assert_eq!(
        response.context.as_deref(),
        Some("Backend command coverage")
    );
    assert_eq!(response.scope.as_deref(), Some("src-tauri commands"));
    assert_eq!(response.constraints, vec!["test-only", "no network"]);
    assert_eq!(response.agent_profile_id, "researcher");
    assert_eq!(response.depth_preset, None);
    assert_eq!(response.max_iterations, 7);
    assert_eq!(response.timeout_hours, 1.5);
    assert_eq!(response.checkpoint_interval, 2);
    assert_eq!(response.target_bucket, "command-coverage");
    assert_eq!(response.status, "running");
    assert_eq!(response.current_iteration, 0);
    assert_eq!(response.progress_percentage, 0.0);
    assert!(response.started_at.is_some());
}

#[tokio::test]
async fn start_research_command_rejects_invalid_preset() {
    let app = research_command_app();

    let error = start_research(
        StartResearchInput {
            name: "Bad Research".to_string(),
            question: "Invalid preset?".to_string(),
            context: None,
            scope: None,
            constraints: None,
            agent_profile_id: "researcher".to_string(),
            depth_preset: Some("too-much".to_string()),
            custom_depth: None,
            target_bucket: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("invalid depth preset should error");

    assert!(error.contains("Invalid depth preset: too-much"));
}

#[tokio::test]
async fn test_get_research_process_by_id() {
    let state = setup_test_state();

    let process = create_test_process();
    let id = process.id.clone();

    state.process_repo.create(process).await.unwrap();

    let found = state.process_repo.get_by_id(&id).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "Test Research");
}

#[tokio::test]
async fn get_research_process_command_returns_optional_response() {
    let app = research_command_app();

    let missing = get_research_process("missing-process".to_string(), app.state::<AppState>())
        .await
        .expect("missing process should not error");
    assert!(missing.is_none());

    let process = create_test_process();
    let id = process.id.clone();
    app.state::<AppState>()
        .process_repo
        .create(process)
        .await
        .expect("process creates");

    let response = get_research_process(id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("process lookup should succeed")
        .expect("process should exist");
    assert_eq!(response.id, id.as_str());
    assert_eq!(response.name, "Test Research");
    assert_eq!(response.depth_preset.as_deref(), Some("standard"));
}

#[tokio::test]
async fn test_get_all_research_processes() {
    let state = setup_test_state();

    state
        .process_repo
        .create(create_test_process())
        .await
        .unwrap();

    let brief2 = ResearchBrief::new("Another question");
    let process2 = ResearchProcess::new("Another Research", brief2, "researcher");
    state.process_repo.create(process2).await.unwrap();

    let all = state.process_repo.get_all().await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn get_research_processes_command_filters_by_status_and_rejects_invalid_status() {
    let app = research_command_app();

    let pending = create_test_process();
    app.state::<AppState>()
        .process_repo
        .create(pending)
        .await
        .expect("pending process creates");

    let mut running = create_test_process();
    running.name = "Running Research".to_string();
    running.start();
    app.state::<AppState>()
        .process_repo
        .create(running)
        .await
        .expect("running process creates");

    let all = get_research_processes(None, app.state::<AppState>())
        .await
        .expect("all processes should list");
    assert_eq!(all.len(), 2);

    let running_only = get_research_processes(Some("running".to_string()), app.state::<AppState>())
        .await
        .expect("running processes should list");
    assert_eq!(running_only.len(), 1);
    assert_eq!(running_only[0].name, "Running Research");

    let error = get_research_processes(Some("unknown".to_string()), app.state::<AppState>())
        .await
        .expect_err("invalid status should error");
    assert!(error.contains("Invalid status: unknown"));
}

#[tokio::test]
async fn test_pause_and_resume_research() {
    let state = setup_test_state();

    let mut process = create_test_process();
    process.start();
    let id = process.id.clone();

    state.process_repo.create(process).await.unwrap();

    // Get and pause
    let mut found = state.process_repo.get_by_id(&id).await.unwrap().unwrap();
    found.pause();
    state.process_repo.update(&found).await.unwrap();

    // Verify paused
    let found = state.process_repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.status(), ResearchProcessStatus::Paused);

    // Resume
    let mut found = state.process_repo.get_by_id(&id).await.unwrap().unwrap();
    found.resume();
    state.process_repo.update(&found).await.unwrap();

    // Verify running
    let found = state.process_repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.status(), ResearchProcessStatus::Running);
}

#[tokio::test]
async fn pause_resume_and_stop_research_commands_enforce_status_transitions() {
    let app = research_command_app();

    let missing_pause = pause_research("missing-process".to_string(), app.state::<AppState>())
        .await
        .expect_err("missing process should error");
    assert!(missing_pause.contains("Research process not found: missing-process"));

    let pending = create_test_process();
    let pending_id = pending.id.clone();
    app.state::<AppState>()
        .process_repo
        .create(pending)
        .await
        .expect("pending process creates");

    let pending_pause = pause_research(pending_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect_err("pending process cannot pause");
    assert!(pending_pause.contains("Cannot pause research in status: pending"));

    let mut running = create_test_process();
    running.start();
    let running_id = running.id.clone();
    app.state::<AppState>()
        .process_repo
        .create(running)
        .await
        .expect("running process creates");

    let paused = pause_research(running_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("running process pauses");
    assert_eq!(paused.status, "paused");

    let paused_again = pause_research(running_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect_err("paused process cannot pause again");
    assert!(paused_again.contains("Cannot pause research in status: paused"));

    let resumed = resume_research(running_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("paused process resumes");
    assert_eq!(resumed.status, "running");

    let resume_running = resume_research(running_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect_err("running process cannot resume");
    assert!(resume_running.contains("Cannot resume research in status: running"));

    let stopped = stop_research(running_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("running process stops");
    assert_eq!(stopped.status, "failed");
    assert_eq!(stopped.error_message.as_deref(), Some("Stopped by user"));

    let stop_terminal = stop_research(running_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect_err("terminal process cannot stop again");
    assert!(stop_terminal.contains("Research process already completed with status: failed"));
}

#[tokio::test]
async fn test_complete_research_process() {
    let state = setup_test_state();

    let mut process = create_test_process();
    process.start();
    let id = process.id.clone();

    state.process_repo.create(process).await.unwrap();

    state.process_repo.complete(&id).await.unwrap();

    let found = state.process_repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.status(), ResearchProcessStatus::Completed);
}

#[tokio::test]
async fn test_fail_research_process() {
    let state = setup_test_state();

    let mut process = create_test_process();
    process.start();
    let id = process.id.clone();

    state.process_repo.create(process).await.unwrap();

    state.process_repo.fail(&id, "Test error").await.unwrap();

    let found = state.process_repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.status(), ResearchProcessStatus::Failed);
}

#[tokio::test]
async fn test_get_processes_by_status() {
    let state = setup_test_state();

    // Create pending process
    let process1 = create_test_process();
    state.process_repo.create(process1).await.unwrap();

    // Create running process
    let mut process2 = create_test_process();
    process2.start();
    state.process_repo.create(process2).await.unwrap();

    // Get pending only
    let pending = state
        .process_repo
        .get_by_status(ResearchProcessStatus::Pending)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);

    // Get running only
    let running = state
        .process_repo
        .get_by_status(ResearchProcessStatus::Running)
        .await
        .unwrap();
    assert_eq!(running.len(), 1);
}

#[tokio::test]
async fn test_research_process_response_serialization() {
    let mut process = create_test_process();
    process.start();
    process.progress.current_iteration = 10;

    let response = ResearchProcessResponse::from(process);

    assert_eq!(response.name, "Test Research");
    assert_eq!(response.status, "running");
    assert_eq!(response.current_iteration, 10);
    assert!(response.depth_preset.is_some());
    assert_eq!(response.depth_preset.as_ref().unwrap(), "standard");

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"name\":\"Test Research\""));
}

#[tokio::test]
async fn test_get_research_presets() {
    let result = get_research_presets().await.unwrap();

    assert_eq!(result.len(), 4);

    let ids: Vec<&str> = result.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.contains(&"quick-scan"));
    assert!(ids.contains(&"standard"));
    assert!(ids.contains(&"deep-dive"));
    assert!(ids.contains(&"exhaustive"));

    let standard = result.iter().find(|p| p.id == "standard").unwrap();
    assert_eq!(standard.max_iterations, 50);
    assert_eq!(standard.timeout_hours, 2.0);
}

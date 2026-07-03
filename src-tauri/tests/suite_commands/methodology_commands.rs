use ralphx_lib::application::AppState;
use ralphx_lib::commands::methodology_commands::{
    activate_methodology, deactivate_methodology, get_active_methodology, get_methodologies,
    MethodologyActivationResponse, MethodologyResponse,
};
use ralphx_lib::commands::WorkflowSchemaResponse;
use ralphx_lib::domain::entities::methodology::MethodologyExtension;
use ralphx_lib::domain::entities::status::InternalStatus;
use ralphx_lib::domain::entities::workflow::{WorkflowColumn, WorkflowSchema};
use tauri::Manager;

fn setup_test_state() -> AppState {
    AppState::new_test()
}

fn methodology_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

fn create_test_workflow() -> WorkflowSchema {
    WorkflowSchema::new(
        "Test Workflow",
        vec![
            WorkflowColumn::new("backlog", "Backlog", InternalStatus::Backlog),
            WorkflowColumn::new("in_progress", "In Progress", InternalStatus::Executing),
            WorkflowColumn::new("done", "Done", InternalStatus::Approved),
        ],
    )
}

fn create_test_methodology() -> MethodologyExtension {
    MethodologyExtension::new("Test Method", create_test_workflow())
        .with_description("A test methodology")
        .with_agent_profiles(["analyst", "developer"])
        .with_skills(["skill1", "skill2"])
}

// ===== get_methodologies Tests =====

#[tokio::test]
async fn test_get_methodologies_empty() {
    let state = setup_test_state();

    let result = state.methodology_repo.get_all().await.unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_get_methodologies_returns_all() {
    let state = setup_test_state();

    // Add two methodologies
    let m1 = create_test_methodology();
    let mut m2 = create_test_methodology();
    m2.name = "Second Method".to_string();

    state.methodology_repo.create(m1).await.unwrap();
    state.methodology_repo.create(m2).await.unwrap();

    let result = state.methodology_repo.get_all().await.unwrap();
    assert_eq!(result.len(), 2);
}

#[tokio::test]
async fn get_methodologies_command_maps_repository_rows() {
    let app = methodology_command_app();

    let methodology = create_test_methodology();
    app.state::<AppState>()
        .methodology_repo
        .create(methodology)
        .await
        .expect("methodology creates");

    let response = get_methodologies(app.state::<AppState>())
        .await
        .expect("methodologies should list");

    assert_eq!(response.len(), 1);
    assert_eq!(response[0].name, "Test Method");
    assert_eq!(response[0].workflow_name, "Test Workflow");
    assert_eq!(response[0].agent_profiles, vec!["analyst", "developer"]);
    assert_eq!(response[0].skills, vec!["skill1", "skill2"]);
    assert_eq!(response[0].phase_count, 0);
    assert_eq!(response[0].agent_count, 2);
}

// ===== get_active_methodology Tests =====

#[tokio::test]
async fn test_get_active_methodology_none() {
    let state = setup_test_state();

    let methodology = create_test_methodology();
    state.methodology_repo.create(methodology).await.unwrap();

    let result = state.methodology_repo.get_active().await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_active_methodology_some() {
    let state = setup_test_state();

    let methodology = create_test_methodology();
    let id = methodology.id.clone();
    state.methodology_repo.create(methodology).await.unwrap();
    state.methodology_repo.activate(&id).await.unwrap();

    let result = state.methodology_repo.get_active().await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().id, id);
}

#[tokio::test]
async fn get_active_methodology_command_returns_none_then_active_response() {
    let app = methodology_command_app();

    let none = get_active_methodology(app.state::<AppState>())
        .await
        .expect("active lookup should not error");
    assert!(none.is_none());

    let methodology = create_test_methodology();
    let id = methodology.id.clone();
    app.state::<AppState>()
        .methodology_repo
        .create(methodology)
        .await
        .expect("methodology creates");
    app.state::<AppState>()
        .methodology_repo
        .activate(&id)
        .await
        .expect("methodology activates");

    let active = get_active_methodology(app.state::<AppState>())
        .await
        .expect("active lookup should return response")
        .expect("active methodology exists");
    assert_eq!(active.id, id.as_str());
    assert!(active.is_active);
}

// ===== activate_methodology Tests =====

#[tokio::test]
async fn test_activate_methodology_success() {
    let state = setup_test_state();

    let methodology = create_test_methodology();
    let id = methodology.id.clone();
    state.methodology_repo.create(methodology).await.unwrap();

    // Activate
    state.methodology_repo.activate(&id).await.unwrap();

    // Verify active
    let active = state.methodology_repo.get_active().await.unwrap();
    assert!(active.is_some());
    assert!(active.unwrap().is_active);
}

#[tokio::test]
async fn test_activate_methodology_deactivates_previous() {
    let state = setup_test_state();

    // Create and activate first methodology
    let m1 = create_test_methodology();
    let id1 = m1.id.clone();
    state.methodology_repo.create(m1).await.unwrap();
    state.methodology_repo.activate(&id1).await.unwrap();

    // Create second methodology
    let mut m2 = create_test_methodology();
    m2.name = "Second Method".to_string();
    let id2 = m2.id.clone();
    state.methodology_repo.create(m2).await.unwrap();

    // Deactivate first, activate second
    state.methodology_repo.deactivate(&id1).await.unwrap();
    state.methodology_repo.activate(&id2).await.unwrap();

    // Verify second is active
    let active = state.methodology_repo.get_active().await.unwrap();
    assert!(active.is_some());
    assert_eq!(active.unwrap().id, id2);

    // Verify first is not active
    let m1_now = state
        .methodology_repo
        .get_by_id(&id1)
        .await
        .unwrap()
        .unwrap();
    assert!(!m1_now.is_active);
}

#[tokio::test]
async fn activate_methodology_command_deactivates_previous_and_reports_context() {
    let app = methodology_command_app();

    let first = create_test_methodology();
    let first_id = first.id.clone();
    app.state::<AppState>()
        .methodology_repo
        .create(first)
        .await
        .expect("first methodology creates");
    app.state::<AppState>()
        .methodology_repo
        .activate(&first_id)
        .await
        .expect("first methodology activates");

    let mut second = create_test_methodology();
    second.name = "Second Method".to_string();
    let second_id = second.id.clone();
    app.state::<AppState>()
        .methodology_repo
        .create(second)
        .await
        .expect("second methodology creates");

    let response = activate_methodology(second_id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("second activation should succeed");

    assert_eq!(response.previous_methodology_id.as_deref(), Some(first_id.as_str()));
    assert_eq!(response.methodology.id, second_id.as_str());
    assert!(response.methodology.is_active);
    assert_eq!(response.workflow.name, "Test Workflow");
    assert_eq!(response.workflow.column_count, 3);
    assert_eq!(response.agent_profiles, vec!["analyst", "developer"]);
    assert_eq!(response.skills, vec!["skill1", "skill2"]);

    let first_after = app
        .state::<AppState>()
        .methodology_repo
        .get_by_id(&first_id)
        .await
        .expect("first lookup")
        .expect("first methodology still exists");
    assert!(!first_after.is_active);
}

#[tokio::test]
async fn activate_methodology_command_rejects_missing_and_already_active() {
    let app = methodology_command_app();

    let missing_error = activate_methodology("missing-method".to_string(), app.state::<AppState>())
        .await
        .expect_err("missing methodology should error");
    assert!(missing_error.contains("Methodology not found: missing-method"));

    let methodology = create_test_methodology();
    let id = methodology.id.clone();
    app.state::<AppState>()
        .methodology_repo
        .create(methodology)
        .await
        .expect("methodology creates");
    activate_methodology(id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("first activation succeeds");

    let active_error = activate_methodology(id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect_err("already active methodology should error");
    assert!(active_error.contains("already active"));
}

// ===== deactivate_methodology Tests =====

#[tokio::test]
async fn test_deactivate_methodology_success() {
    let state = setup_test_state();

    let methodology = create_test_methodology();
    let id = methodology.id.clone();
    state.methodology_repo.create(methodology).await.unwrap();
    state.methodology_repo.activate(&id).await.unwrap();

    // Deactivate
    state.methodology_repo.deactivate(&id).await.unwrap();

    // Verify no active methodology
    let active = state.methodology_repo.get_active().await.unwrap();
    assert!(active.is_none());

    // Verify methodology is not active
    let methodology = state
        .methodology_repo
        .get_by_id(&id)
        .await
        .unwrap()
        .unwrap();
    assert!(!methodology.is_active);
}

#[tokio::test]
async fn deactivate_methodology_command_rejects_missing_inactive_and_clears_active() {
    let app = methodology_command_app();

    let missing_error =
        deactivate_methodology("missing-method".to_string(), app.state::<AppState>())
            .await
            .expect_err("missing methodology should error");
    assert!(missing_error.contains("Methodology not found: missing-method"));

    let methodology = create_test_methodology();
    let id = methodology.id.clone();
    app.state::<AppState>()
        .methodology_repo
        .create(methodology)
        .await
        .expect("methodology creates");

    let inactive_error = deactivate_methodology(id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect_err("inactive methodology should error");
    assert!(inactive_error.contains("is not active"));

    app.state::<AppState>()
        .methodology_repo
        .activate(&id)
        .await
        .expect("methodology activates");
    let response = deactivate_methodology(id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("active methodology deactivates");
    assert_eq!(response.id, id.as_str());
    assert!(!response.is_active);

    let active = get_active_methodology(app.state::<AppState>())
        .await
        .expect("active lookup succeeds");
    assert!(active.is_none());
}

// ===== Response Serialization Tests =====

#[test]
fn test_methodology_response_serialization() {
    let methodology = create_test_methodology();
    let response = MethodologyResponse::from(methodology);

    assert_eq!(response.name, "Test Method");
    assert_eq!(response.description, Some("A test methodology".to_string()));
    assert_eq!(response.agent_profiles.len(), 2);
    assert_eq!(response.skills.len(), 2);
    assert!(!response.is_active);
    assert_eq!(response.phase_count, 0);
    assert_eq!(response.agent_count, 2);

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"name\":\"Test Method\""));
    assert!(json.contains("\"is_active\":false"));
}

#[test]
fn test_methodology_activation_response_serialization() {
    let methodology = create_test_methodology();
    let workflow = methodology.workflow.clone();

    let response = MethodologyActivationResponse {
        methodology: MethodologyResponse::from(methodology),
        workflow: WorkflowSchemaResponse::from(&workflow),
        agent_profiles: vec!["analyst".to_string(), "developer".to_string()],
        skills: vec!["skill1".to_string()],
        previous_methodology_id: Some("prev-id".to_string()),
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"agent_profiles\":[\"analyst\",\"developer\"]"));
    assert!(json.contains("\"previous_methodology_id\":\"prev-id\""));
}

#[test]
fn test_builtin_methodologies_response() {
    let bmad = MethodologyExtension::bmad();
    let response = MethodologyResponse::from(bmad);

    assert_eq!(response.id, "bmad-method");
    assert_eq!(response.name, "BMAD Method");
    assert_eq!(response.agent_count, 8);
    assert_eq!(response.phase_count, 4);

    let gsd = MethodologyExtension::gsd();
    let response = MethodologyResponse::from(gsd);

    assert_eq!(response.id, "gsd-method");
    assert_eq!(response.name, "GSD (Get Shit Done)");
    assert_eq!(response.agent_count, 11);
    assert_eq!(response.phase_count, 4);
}

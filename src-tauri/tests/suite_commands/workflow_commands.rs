use ralphx_lib::application::AppState;
use ralphx_lib::commands::workflow_commands::{
    create_workflow, delete_workflow, get_active_workflow_columns, get_builtin_workflows,
    get_workflow, get_workflows, seed_builtin_workflows, set_default_workflow, update_workflow,
    CreateWorkflowInput, UpdateWorkflowInput, WorkflowColumnInput, WorkflowResponse,
};
use ralphx_lib::domain::entities::status::InternalStatus;
use ralphx_lib::domain::entities::workflow::{WorkflowColumn, WorkflowSchema};
use tauri::Manager;

fn setup_test_state() -> AppState {
    AppState::new_test()
}

fn workflow_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

fn workflow_column_input(id: &str, name: &str, maps_to: &str) -> WorkflowColumnInput {
    WorkflowColumnInput {
        id: id.to_string(),
        name: name.to_string(),
        maps_to: maps_to.to_string(),
        color: None,
        icon: None,
        skip_review: None,
        auto_advance: None,
        agent_profile: None,
    }
}

#[tokio::test]
async fn test_create_workflow() {
    let state = setup_test_state();

    let workflow = WorkflowSchema::new(
        "Test Workflow",
        vec![
            WorkflowColumn::new("backlog", "Backlog", InternalStatus::Backlog),
            WorkflowColumn::new("done", "Done", InternalStatus::Approved),
        ],
    );

    let created = state.workflow_repo.create(workflow).await.unwrap();
    assert_eq!(created.name, "Test Workflow");
    assert_eq!(created.columns.len(), 2);
}

#[tokio::test]
async fn create_workflow_command_maps_input_and_persists_response() {
    let app = workflow_command_app();

    let response = create_workflow(
        CreateWorkflowInput {
            name: "Command Workflow".to_string(),
            description: Some("Created through command".to_string()),
            columns: vec![
                WorkflowColumnInput {
                    id: "ready".to_string(),
                    name: "Ready".to_string(),
                    maps_to: "ready".to_string(),
                    color: Some("#00ff00".to_string()),
                    icon: Some("Play".to_string()),
                    skip_review: Some(true),
                    auto_advance: Some(false),
                    agent_profile: Some("fast-worker".to_string()),
                },
                workflow_column_input("done", "Done", "approved"),
            ],
            is_default: Some(true),
            worker_profile: Some("worker".to_string()),
            reviewer_profile: Some("reviewer".to_string()),
            external_sync: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("workflow creates");

    assert_eq!(response.name, "Command Workflow");
    assert_eq!(
        response.description.as_deref(),
        Some("Created through command")
    );
    assert!(response.is_default);
    assert_eq!(response.worker_profile.as_deref(), Some("worker"));
    assert_eq!(response.reviewer_profile.as_deref(), Some("reviewer"));
    assert_eq!(response.columns.len(), 2);
    assert_eq!(response.columns[0].maps_to, "ready");
    assert_eq!(response.columns[0].color.as_deref(), Some("#00ff00"));
    assert_eq!(response.columns[0].icon.as_deref(), Some("Play"));
    assert_eq!(response.columns[0].skip_review, Some(true));
    assert_eq!(response.columns[0].auto_advance, Some(false));
    assert_eq!(
        response.columns[0].agent_profile.as_deref(),
        Some("fast-worker")
    );

    let stored = get_workflow(response.id.clone(), app.state::<AppState>())
        .await
        .expect("workflow lookup should succeed")
        .expect("created workflow should exist");
    assert_eq!(stored.name, "Command Workflow");
}

#[tokio::test]
async fn create_workflow_command_rejects_invalid_column_status() {
    let app = workflow_command_app();

    let error = create_workflow(
        CreateWorkflowInput {
            name: "Bad Workflow".to_string(),
            description: None,
            columns: vec![workflow_column_input("bad", "Bad", "not_real")],
            is_default: None,
            worker_profile: None,
            reviewer_profile: None,
            external_sync: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("invalid status should error");

    assert!(error.contains("Invalid internal status: not_real"));
}

#[tokio::test]
async fn test_get_workflow_by_id() {
    let state = setup_test_state();

    let workflow = WorkflowSchema::new(
        "Find Me",
        vec![WorkflowColumn::new("col", "Column", InternalStatus::Ready)],
    );
    let id = workflow.id.clone();

    state.workflow_repo.create(workflow).await.unwrap();

    let found = state.workflow_repo.get_by_id(&id).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "Find Me");
}

#[tokio::test]
async fn get_workflow_command_returns_optional_response() {
    let app = workflow_command_app();

    let missing = get_workflow("missing-workflow".to_string(), app.state::<AppState>())
        .await
        .expect("missing workflow should not error");
    assert!(missing.is_none());

    let workflow = WorkflowSchema::new(
        "Find Via Command",
        vec![WorkflowColumn::new("ready", "Ready", InternalStatus::Ready)],
    );
    let id = workflow.id.clone();
    app.state::<AppState>()
        .workflow_repo
        .create(workflow)
        .await
        .expect("workflow creates");

    let response = get_workflow(id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("workflow lookup should succeed")
        .expect("workflow should exist");
    assert_eq!(response.id, id.as_str());
    assert_eq!(response.name, "Find Via Command");
}

#[tokio::test]
async fn test_list_workflows() {
    let state = setup_test_state();

    state
        .workflow_repo
        .create(WorkflowSchema::new(
            "WF 1",
            vec![WorkflowColumn::new("a", "A", InternalStatus::Backlog)],
        ))
        .await
        .unwrap();
    state
        .workflow_repo
        .create(WorkflowSchema::new(
            "WF 2",
            vec![WorkflowColumn::new("b", "B", InternalStatus::Ready)],
        ))
        .await
        .unwrap();

    let all = state.workflow_repo.get_all().await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn get_workflows_command_maps_all_rows() {
    let app = workflow_command_app();

    app.state::<AppState>()
        .workflow_repo
        .create(WorkflowSchema::new(
            "WF Command 1",
            vec![WorkflowColumn::new("a", "A", InternalStatus::Backlog)],
        ))
        .await
        .expect("first workflow creates");
    app.state::<AppState>()
        .workflow_repo
        .create(WorkflowSchema::new(
            "WF Command 2",
            vec![WorkflowColumn::new("b", "B", InternalStatus::Ready)],
        ))
        .await
        .expect("second workflow creates");

    let workflows = get_workflows(app.state::<AppState>())
        .await
        .expect("workflows should list");
    let names: Vec<&str> = workflows
        .iter()
        .map(|workflow| workflow.name.as_str())
        .collect();
    assert_eq!(workflows.len(), 2);
    assert!(names.contains(&"WF Command 1"));
    assert!(names.contains(&"WF Command 2"));
}

#[tokio::test]
async fn test_set_default_workflow() {
    let state = setup_test_state();

    let wf1 = WorkflowSchema::default_ralphx();
    let wf2 = WorkflowSchema::new(
        "Second",
        vec![WorkflowColumn::new("x", "X", InternalStatus::Backlog)],
    );
    let wf2_id = wf2.id.clone();

    state.workflow_repo.create(wf1).await.unwrap();
    state.workflow_repo.create(wf2).await.unwrap();

    state.workflow_repo.set_default(&wf2_id).await.unwrap();

    let default = state.workflow_repo.get_default().await.unwrap();
    assert!(default.is_some());
    assert_eq!(default.unwrap().id, wf2_id);
}

#[tokio::test]
async fn update_workflow_command_applies_partial_updates_and_validates_missing_rows() {
    let app = workflow_command_app();

    let missing_error = update_workflow(
        "missing-workflow".to_string(),
        UpdateWorkflowInput {
            name: Some("Nope".to_string()),
            description: None,
            columns: None,
            is_default: None,
            worker_profile: None,
            reviewer_profile: None,
            external_sync: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("missing workflow should error");
    assert!(missing_error.contains("Workflow not found: missing-workflow"));

    let workflow = WorkflowSchema::new(
        "Original Workflow",
        vec![WorkflowColumn::new("ready", "Ready", InternalStatus::Ready)],
    );
    let id = workflow.id.clone();
    app.state::<AppState>()
        .workflow_repo
        .create(workflow)
        .await
        .expect("workflow creates");

    let invalid_error = update_workflow(
        id.as_str().to_string(),
        UpdateWorkflowInput {
            name: None,
            description: None,
            columns: Some(vec![workflow_column_input("bad", "Bad", "not_real")]),
            is_default: None,
            worker_profile: None,
            reviewer_profile: None,
            external_sync: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("invalid column update should error");
    assert!(invalid_error.contains("Invalid internal status: not_real"));

    let response = update_workflow(
        id.as_str().to_string(),
        UpdateWorkflowInput {
            name: Some("Updated Workflow".to_string()),
            description: Some("Updated description".to_string()),
            columns: Some(vec![
                workflow_column_input("backlog", "Backlog", "backlog"),
                workflow_column_input("approved", "Approved", "approved"),
            ]),
            is_default: Some(true),
            worker_profile: Some("worker-updated".to_string()),
            reviewer_profile: Some("reviewer-updated".to_string()),
            external_sync: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("workflow updates");

    assert_eq!(response.name, "Updated Workflow");
    assert_eq!(response.description.as_deref(), Some("Updated description"));
    assert!(response.is_default);
    assert_eq!(response.worker_profile.as_deref(), Some("worker-updated"));
    assert_eq!(
        response.reviewer_profile.as_deref(),
        Some("reviewer-updated")
    );
    assert_eq!(response.columns.len(), 2);
    assert_eq!(response.columns[0].maps_to, "backlog");
}

#[tokio::test]
async fn delete_workflow_command_removes_existing_workflow() {
    let app = workflow_command_app();
    let workflow = WorkflowSchema::new(
        "Delete Via Command",
        vec![WorkflowColumn::new("ready", "Ready", InternalStatus::Ready)],
    );
    let id = workflow.id.clone();
    app.state::<AppState>()
        .workflow_repo
        .create(workflow)
        .await
        .expect("workflow creates");

    delete_workflow(id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("workflow deletes");

    let missing = get_workflow(id.as_str().to_string(), app.state::<AppState>())
        .await
        .expect("lookup after delete should not error");
    assert!(missing.is_none());
}

#[tokio::test]
async fn seed_and_set_default_workflow_commands_are_idempotent() {
    let app = workflow_command_app();

    let created = seed_builtin_workflows(app.state::<AppState>())
        .await
        .expect("builtins seed");
    assert_eq!(created, 3);

    let created_again = seed_builtin_workflows(app.state::<AppState>())
        .await
        .expect("builtins seed idempotently");
    assert_eq!(created_again, 0);

    let workflows = get_workflows(app.state::<AppState>())
        .await
        .expect("seeded workflows list");
    let jira = workflows
        .iter()
        .find(|workflow| workflow.name == "Jira Compatible")
        .expect("jira builtin exists");

    let response = set_default_workflow(jira.id.clone(), app.state::<AppState>())
        .await
        .expect("default workflow sets");
    assert_eq!(response.name, "Jira Compatible");
    assert!(response.is_default);
}

#[tokio::test]
async fn test_workflow_response_serialization() {
    let workflow = WorkflowSchema::new(
        "Response Test",
        vec![
            WorkflowColumn::new("col1", "Column 1", InternalStatus::Backlog).with_color("#ff0000"),
        ],
    )
    .with_description("A test workflow");

    let response = WorkflowResponse::from(workflow);

    assert_eq!(response.name, "Response Test");
    assert_eq!(response.description, Some("A test workflow".to_string()));
    assert_eq!(response.columns.len(), 1);
    assert_eq!(response.columns[0].color, Some("#ff0000".to_string()));

    // Verify JSON serialization uses snake_case (Rust default)
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"name\":\"Response Test\""));
    assert!(
        json.contains("\"is_default\""),
        "Expected snake_case field is_default"
    );
}

#[tokio::test]
async fn test_column_input_to_column() {
    let input = WorkflowColumnInput {
        id: "test-col".to_string(),
        name: "Test Column".to_string(),
        maps_to: "ready".to_string(),
        color: Some("#00ff00".to_string()),
        icon: None,
        skip_review: Some(true),
        auto_advance: None,
        agent_profile: Some("fast-worker".to_string()),
    };

    let column = input.to_column().unwrap();

    assert_eq!(column.id, "test-col");
    assert_eq!(column.name, "Test Column");
    assert_eq!(column.maps_to, InternalStatus::Ready);
    assert_eq!(column.color, Some("#00ff00".to_string()));

    let behavior = column.behavior.unwrap();
    assert_eq!(behavior.skip_review, Some(true));
    assert_eq!(behavior.agent_profile, Some("fast-worker".to_string()));
}

#[tokio::test]
async fn test_column_input_invalid_status() {
    let input = WorkflowColumnInput {
        id: "test".to_string(),
        name: "Test".to_string(),
        maps_to: "invalid_status".to_string(),
        color: None,
        icon: None,
        skip_review: None,
        auto_advance: None,
        agent_profile: None,
    };

    let result = input.to_column();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid internal status"));
}

#[tokio::test]
async fn test_get_builtin_workflows() {
    let result = get_builtin_workflows().await.unwrap();

    assert_eq!(result.len(), 3);

    let names: Vec<&str> = result.iter().map(|w| w.name.as_str()).collect();
    assert!(names.contains(&"RalphX Default"));
    assert!(names.contains(&"Jira Compatible"));
    assert!(names.contains(&"Linear Compatible"));
}

#[tokio::test]
async fn test_get_active_workflow_columns_with_default() {
    let state = setup_test_state();

    // Create and set a default workflow
    let workflow = WorkflowSchema::new(
        "My Default",
        vec![
            WorkflowColumn::new("a", "A", InternalStatus::Backlog),
            WorkflowColumn::new("b", "B", InternalStatus::Approved),
        ],
    )
    .as_default();
    let _id = workflow.id.clone();

    state.workflow_repo.create(workflow).await.unwrap();

    let default = state.workflow_repo.get_default().await.unwrap();
    assert!(default.is_some());
    assert_eq!(default.unwrap().columns.len(), 2);
}

#[tokio::test]
async fn get_active_workflow_columns_command_uses_default_or_fallback() {
    let app = workflow_command_app();

    let fallback = get_active_workflow_columns(app.state::<AppState>())
        .await
        .expect("fallback columns should load");
    assert!(!fallback.is_empty());
    assert!(fallback.iter().any(|column| column.id == "ready"));

    let workflow = WorkflowSchema::new(
        "Command Default",
        vec![
            WorkflowColumn::new("custom-a", "Custom A", InternalStatus::Backlog),
            WorkflowColumn::new("custom-b", "Custom B", InternalStatus::Approved),
        ],
    )
    .as_default();
    app.state::<AppState>()
        .workflow_repo
        .create(workflow)
        .await
        .expect("default workflow creates");

    let columns = get_active_workflow_columns(app.state::<AppState>())
        .await
        .expect("default columns should load");
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0].id, "custom-a");
    assert_eq!(columns[1].id, "custom-b");
}

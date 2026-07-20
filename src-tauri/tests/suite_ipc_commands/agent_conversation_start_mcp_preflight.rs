use super::agent_conversation_start_support::*;

#[tokio::test]
async fn mcp_preflight_conflict_fails_before_conversation_or_workspace_writes() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("project fixture");
    let provider_home = tempfile::tempdir().expect("provider home fixture");
    let app_data = tempfile::tempdir().expect("app data fixture");
    std::fs::create_dir(provider_home.path().join(".ralphx")).unwrap();
    std::fs::write(
        provider_home.path().join(".claude.json"),
        r#"{"mcpServers":{"ralphx":{"command":"user-owned","env":{"TOKEN":"secret"}}}}"#,
    )
    .unwrap();
    let mut state = AppState::new_test();
    state.app_paths =
        AppPaths::new_with_config_dir(app_data.path(), None, provider_home.path().join(".ralphx"));
    let project = seed_project(
        &state,
        "project-mcp-preflight-conflict",
        temp.path(),
        temp.path(),
    )
    .await;
    let app = build_app(state, Arc::new(ExecutionState::new()));
    let mut input = service_start_input(
        &project.id,
        "This must not create side effects",
        "edit",
        None,
        None,
        None,
        None,
    );
    input.provider_harness = Some("claude".to_string());

    let error = start_with_app(&app, input)
        .await
        .expect_err("reserved MCP conflict must fail early");

    assert!(error.contains("[ralphx:mcp_setup_preflight]"));
    assert!(!error.contains("TOKEN"));
    assert!(app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_context(ChatContextType::Project, project.id.as_str())
        .await
        .unwrap()
        .is_empty());
    assert!(app
        .state::<AppState>()
        .agent_conversation_workspace_repo
        .get_by_project_id(&project.id)
        .await
        .unwrap()
        .is_empty());
}

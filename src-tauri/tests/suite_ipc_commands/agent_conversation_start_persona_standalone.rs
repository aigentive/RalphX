use super::agent_conversation_start_support::*;

#[tokio::test]
async fn standalone_persona_builder_uses_workspace_cwd_and_filesystem_enforcement() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(true));
    let _allow_spawn =
        super::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let result = start_with_app(
        &app,
        standalone_start_input(
            "Build a global persona",
            Some("persona_builder"),
            None,
            None,
            None,
        ),
    )
    .await
    .expect("standalone Claude-lane builder should start");
    let app_data_dir = app
        .state::<AppState>()
        .app_paths
        .app_data_dir()
        .to_path_buf();
    let expected_workspace = standalone_workspace_path(
        &standalone_workspaces_root(&app_data_dir),
        &result.conversation.id.as_str(),
    );
    let captured = fake_cli.captured_prompt().await;
    assert!(
        captured.contains(expected_workspace.to_string_lossy().as_ref()),
        "spawn must run from or expose the private workspace: {captured}"
    );
    assert!(
        captured.contains("--filesystem-enforced") && captured.contains("\"1\""),
        "builder spawn must enable filesystem enforcement: {captured}"
    );
}

#[tokio::test]
async fn standalone_builder_seed_with_folder_reference_starts_with_enforced_root() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    set_composer_folder_references_override(Some(true));
    set_standalone_conversations_override(Some(true));
    let _allow_spawn =
        super::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("standalone builder folder-reference fixture");
    let app_data_dir = validate_absolute_non_root_path(
        &temp.path().join("app-data"),
        "standalone builder app data",
    )
    .expect("safe standalone builder app data");
    let folder_root = validate_absolute_non_root_path(
        &temp.path().join("folder-root"),
        "standalone builder folder root",
    )
    .expect("safe standalone builder folder root");
    std::fs::create_dir_all(&folder_root).expect("create folder-reference root");
    let mut state = AppState::new_test();
    state.app_paths = AppPaths::new(app_data_dir, None);
    let app = build_app(state, Arc::new(ExecutionState::new()));
    let created = create_agent_conversation(
        CreateAgentConversationInput {
            context_type: "standalone".to_string(),
            context_id: None,
            title: Some("Global persona builder".to_string()),
            mode: Some("persona_builder".to_string()),
            team_intent: None,
        },
        app.state(),
    )
    .await
    .expect("production builder seed succeeds");
    let conversation_id = ChatConversationId::from_string(created.id);
    add_conversation_folder_reference_for_state(
        AddConversationFolderReferenceInput {
            conversation_id: conversation_id.as_str(),
            folder_path: folder_root.to_string_lossy().into_owned(),
            display_name: "Folder root".to_string(),
        },
        app.state::<AppState>().inner(),
        true,
    )
    .await
    .expect("pre-start folder reference registration succeeds");

    start_with_app(
        &app,
        standalone_start_input(
            "Build from the registered folder",
            Some("persona_builder"),
            Some(&conversation_id),
            None,
            None,
        ),
    )
    .await
    .expect("standalone builder with folder reference starts");

    let captured = fake_cli.captured_prompt().await;
    assert!(
        captured.contains("--filesystem-enforced") && captured.contains("\"1\""),
        "builder spawn must enable filesystem enforcement: {captured}"
    );
    assert!(
        captured.contains(folder_root.to_string_lossy().as_ref()),
        "registered folder reference must be present in enforced roots: {captured}"
    );
}

#[tokio::test]
async fn standalone_chat_rejects_codex_while_project_chat_allows_codex() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let db = SqliteTestDb::new("seeded-refine-standard-start");
    let shared = db.shared_conn();
    let mut state = AppState::new_test();
    state.db = DbConnection::from_shared(Arc::clone(&shared));
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(shared));
    let project = seed_project(&state, "project-chat-codex", temp.path(), temp.path()).await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let mut standalone = standalone_start_input(
        "Reject unsafe standalone lane",
        Some("chat"),
        None,
        None,
        None,
    );
    standalone.provider_harness = Some("codex".to_string());
    let error = start_with_app(&app, standalone)
        .await
        .expect_err("standalone chat must reject Codex");
    assert!(
        error.contains("Claude harness"),
        "unexpected error: {error}"
    );

    let mut project_input = service_start_input(
        &project.id,
        "Project Codex chat remains project bounded",
        "chat",
        None,
        None,
        None,
        None,
    );
    project_input.provider_harness = Some("codex".to_string());
    let started = start_with_app(&app, project_input)
        .await
        .expect("Project-context Codex chat remains allowed");
    assert!(started.send_result.was_queued);
}

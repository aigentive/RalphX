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
async fn standalone_persona_builder_start_accepts_codex_and_persists_provider_mode() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(true));
    let fake_codex = FakeCodex::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_codex
            .cli_path
            .to_str()
            .expect("fake Codex path should be UTF-8"),
    );
    let state = AppState::new_test();
    state
        .manual_role_default_repo
        .upsert_global(
            RoutingRole::UtilityLightweight,
            &manual_role_default(AgentHarnessKind::Claude),
        )
        .await
        .expect("global Claude utility default should persist");
    let app = build_app(state, Arc::new(ExecutionState::new()));
    let mut input = standalone_start_input(
        "Build a global persona with Codex",
        Some("persona_builder"),
        None,
        None,
        None,
    );
    input.provider_harness = Some("codex".to_string());

    let result = start_with_app(&app, input)
        .await
        .expect("standalone PersonaBuilder Codex start should be accepted");

    let persisted = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&result.conversation.id)
        .await
        .expect("PersonaBuilder conversation lookup should succeed")
        .expect("PersonaBuilder conversation should persist");
    assert_eq!(
        persisted.agent_mode,
        Some(AgentConversationWorkspaceMode::PersonaBuilder),
    );
    let runs = app
        .state::<AppState>()
        .agent_run_repo
        .get_by_conversation(&result.conversation.id)
        .await
        .expect("PersonaBuilder Codex run lookup should succeed");
    assert!(
        runs.iter()
            .any(|run| run.harness == Some(AgentHarnessKind::Codex)),
        "start must persist the explicit Codex provider on the builder run",
    );
    assert!(
        fake_codex.was_invoked(),
        "Standalone PersonaBuilder start must invoke Codex",
    );
}

#[tokio::test]
async fn standalone_builder_seed_with_folder_reference_starts_with_enforced_root() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
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

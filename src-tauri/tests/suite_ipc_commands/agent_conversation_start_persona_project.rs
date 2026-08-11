use super::agent_conversation_start_support::*;

#[tokio::test]
async fn start_agent_conversation_persona_builder_flag_off_is_rejected() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(false));
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let project_id = ProjectId::from_string("project-persona-builder-flag-off".to_string());

    let error = start_with_app(
        &app,
        service_start_input(
            &project_id,
            "flag-off builder must not start",
            "persona_builder",
            None,
            None,
            None,
            None,
        ),
    )
    .await
    .expect_err("builder start must reject while agent_personas is disabled");

    assert!(
        error.contains("agent_personas"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn all_flags_on_project_persona_builder_succeeds_through_standard_pipeline() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let db = SqliteTestDb::new("seeded-refine-scope-lock");
    let shared = db.shared_conn();
    let mut state = AppState::new_test();
    state.db = DbConnection::from_shared(Arc::clone(&shared));
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(shared));
    let isolated_app_data = temp.path().join("app-data");
    std::fs::create_dir(&isolated_app_data).expect("create isolated app data");
    state.app_paths = AppPaths::new(isolated_app_data, None);
    state.attachment_storage_path = state.app_paths.attachment_storage_path();
    let project = seed_project(
        &state,
        "project-persona-builder-start",
        temp.path(),
        temp.path(),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let started = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Interview me before drafting",
            "persona_builder",
            None,
            None,
            None,
            None,
        ),
    )
    .await
    .expect("project builder should start through the standard pipeline");

    assert_eq!(
        started.conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::PersonaBuilder)
    );
    assert!(started.send_result.was_queued);

    let app_data_dir = app
        .state::<AppState>()
        .app_paths
        .app_data_dir()
        .to_path_buf();
    let workspace = standalone_workspace_path(
        &standalone_workspaces_root(&app_data_dir),
        &started.conversation.id.as_str(),
    );
    assert!(
        workspace.join("manifest.json").is_file(),
        "Project builders must receive the same private workspace as standalone builders"
    );
    let summary = sweep_orphaned_standalone_workspaces(
        &app_data_dir,
        Arc::clone(&app.state::<AppState>().chat_conversation_repo),
    )
    .await;
    assert!(summary.retained >= 1);
    assert!(
        workspace.is_dir(),
        "a live Project builder workspace must survive the sweep"
    );
    app.state::<AppState>()
        .chat_conversation_repo
        .delete(&started.conversation.id)
        .await
        .expect("delete Project builder conversation");
    let orphan_summary = sweep_orphaned_standalone_workspaces(
        &app_data_dir,
        Arc::clone(&app.state::<AppState>().chat_conversation_repo),
    )
    .await;
    assert!(orphan_summary.removed >= 1);
    assert!(
        !workspace.exists(),
        "the same Project builder workspace must sweep once its conversation row is absent"
    );
}

#[tokio::test]
async fn project_builder_still_materializes_text_attachment() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let db = SqliteTestDb::new("builder-start-attachment-sync");
    let shared = db.shared_conn();
    let mut state = AppState::new_test();
    state.db = DbConnection::from_shared(Arc::clone(&shared));
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(shared));
    let project = seed_project(
        &state,
        "project-builder-attachment-sync",
        temp.path(),
        temp.path(),
    )
    .await;
    let mut seeded = ChatConversation::new_project(project.id.clone());
    seeded.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let seeded = state
        .chat_conversation_repo
        .create(seeded)
        .await
        .expect("seed pre-cleanup builder row");
    let attachment = ChatAttachmentService::new(
        Arc::clone(&state.chat_attachment_repo),
        state.attachment_storage_path.clone(),
    )
    .upload(
        &seeded.id,
        "pre-send.txt",
        b"pre-send builder attachment",
        Some("text/plain".to_string()),
    )
    .await
    .expect("seed attachment without attach-time materialization");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let started = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Use the pre-send context",
            "persona_builder",
            None,
            None,
            Some(&seeded.id),
            None,
        ),
    )
    .await
    .expect("seeded Project builder starts");
    assert!(started.send_result.was_queued);
    let materialized = materialized_builder_attachment_path(
        app.state::<AppState>().app_paths.app_data_dir(),
        &attachment,
    )
    .expect("materialized attachment path");
    assert_eq!(
        std::fs::read_to_string(materialized).expect("read materialized text"),
        "pre-send builder attachment"
    );
}

#[tokio::test]
async fn existing_project_builder_persistence_failure_restores_runtime_and_workspace() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let db = SqliteTestDb::new("builder-start-compensation");
    let shared = db.shared_conn();
    let mut state = AppState::new_test();
    state.db = DbConnection::from_shared(Arc::clone(&shared));
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(shared));
    let project = seed_project(
        &state,
        "project-builder-start-compensation",
        temp.path(),
        temp.path(),
    )
    .await;
    let mut seeded = ChatConversation::new_project(project.id.clone());
    seeded.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    seeded.set_coordination_mode(CoordinationMode::Solo);
    let seeded = state
        .chat_conversation_repo
        .create(seeded)
        .await
        .expect("seed existing project conversation");
    db.with_connection(|connection| {
        connection
            .execute_batch(
                "CREATE TRIGGER fail_builder_mode_update
                 BEFORE UPDATE OF agent_mode ON chat_conversations
                 BEGIN SELECT RAISE(ABORT, 'forced mode persistence failure'); END;",
            )
            .expect("mode failure trigger should install");
    });
    let workspace = standalone_workspace_path(
        &standalone_workspaces_root(state.app_paths.app_data_dir()),
        &seeded.id.as_str(),
    );
    let app = build_app(state, Arc::new(ExecutionState::new()));
    let input = service_start_input(
        &project.id,
        "Prepare a builder",
        "persona_builder",
        None,
        None,
        Some(&seeded.id),
        None,
    );
    let error = start_with_app(&app, input)
        .await
        .expect_err("forced mode persistence failure must reject the start");
    assert!(
        error.contains("forced mode persistence failure"),
        "unexpected error: {error}"
    );
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&seeded.id)
        .await
        .expect("conversation lookup should succeed")
        .expect("existing conversation must remain");
    assert_eq!(
        stored.agent_mode,
        Some(AgentConversationWorkspaceMode::Chat),
        "failed preparation must restore the previous mode"
    );
    assert_eq!(
        stored.coordination_mode,
        CoordinationMode::Solo,
        "failed preparation must preserve the previous coordination mode"
    );
    assert!(
        !workspace.exists(),
        "failed preparation must remove the newly created private workspace"
    );
}

#[tokio::test]
async fn persona_builder_binary_attachment_failure_preserves_existing_conversation_without_run() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let db = SqliteTestDb::new("builder-start-binary-attachment-compensation");
    let shared = db.shared_conn();
    let mut state = AppState::new_test();
    state.db = DbConnection::from_shared(Arc::clone(&shared));
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(shared));
    let app_data_dir = temp.path().join("app-data");
    std::fs::create_dir(&app_data_dir).expect("create isolated app data");
    state.app_paths = AppPaths::new(app_data_dir.clone(), None);
    state.attachment_storage_path = state.app_paths.attachment_storage_path();
    let project = seed_project(
        &state,
        "project-builder-binary-attachment",
        temp.path(),
        temp.path(),
    )
    .await;
    let mut seeded = ChatConversation::new_project(project.id.clone());
    seeded.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    let seeded = state
        .chat_conversation_repo
        .create(seeded)
        .await
        .expect("seed existing project conversation");
    let attachment = ChatAttachmentService::new(
        Arc::clone(&state.chat_attachment_repo),
        state.attachment_storage_path.clone(),
    )
    .upload(
        &seeded.id,
        "binary-context.dat",
        &[0, 159, 146, 150],
        Some("application/octet-stream".to_string()),
    )
    .await
    .expect("seed low-level binary attachment fixture");
    let workspace = standalone_workspace_path(
        &standalone_workspaces_root(&app_data_dir),
        &seeded.id.as_str(),
    );
    let app = build_app(state, Arc::new(ExecutionState::new()));

    let error = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Use the binary context",
            "persona_builder",
            None,
            None,
            Some(&seeded.id),
            None,
        ),
    )
    .await
    .expect_err("binary builder attachment must abort preparation");

    assert!(
        error.contains("only read text context"),
        "unexpected materialization error: {error}"
    );
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&seeded.id)
        .await
        .expect("conversation lookup should succeed")
        .expect("existing conversation must remain");
    assert_eq!(
        stored.agent_mode,
        Some(AgentConversationWorkspaceMode::Chat),
        "attachment preparation must fail before changing the persisted mode"
    );
    assert_eq!(stored.coordination_mode, CoordinationMode::Solo);
    assert!(stored.builder_draft_id.is_none());
    assert!(
        !workspace.exists(),
        "failed attachment sync must remove the newly prepared private workspace"
    );
    assert!(
        app.state::<AppState>()
            .agent_run_repo
            .get_by_conversation(&seeded.id)
            .await
            .expect("run lookup should succeed")
            .is_empty(),
        "failed preparation must not dispatch or persist an agent run"
    );
    assert!(
        app.state::<AppState>()
            .chat_attachment_repo
            .get_by_id(&attachment.id)
            .await
            .expect("attachment lookup should succeed")
            .is_some(),
        "failed preparation must preserve the user's source attachment"
    );
}

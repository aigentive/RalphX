use super::agent_conversation_start_support::*;

#[tokio::test]
async fn source_persona_id_rejects_non_builder_mode() {
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let project_id = ProjectId::from_string("project-source-non-builder".to_string());
    let mut input = service_start_input(
        &project_id,
        "Invalid source",
        "chat",
        None,
        None,
        None,
        None,
    );
    input.source_persona_id = Some("persona-source".to_string());
    let error = start_with_app(&app, input)
        .await
        .expect_err("source_persona_id outside builder mode must reject");
    assert!(error.contains("source_persona_id"));
}

#[tokio::test]
async fn persona_seeded_refine_draft_failure_restores_conversation_without_run_or_spawn() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    let fake_codex = FakeCodex::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_codex
            .cli_path
            .to_str()
            .expect("fake Codex path should be UTF-8"),
    );
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let app_data_dir = validate_absolute_non_root_path(
        &temp.path().join("app-data"),
        "seeded refine failure app data",
    )
    .expect("app data should be a safe process-owned path");
    std::fs::create_dir(&app_data_dir).expect("isolated app data should be created");
    let db = SqliteTestDb::new("seeded-refine-draft-failure-restores-start-state");
    let shared = db.shared_conn();
    let mut state = AppState::new_test();
    state.db = DbConnection::from_shared(Arc::clone(&shared));
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(shared));
    state.app_paths = AppPaths::new(app_data_dir, None);
    state.attachment_storage_path = state.app_paths.attachment_storage_path();
    let project = seed_project(
        &state,
        "project-refine-draft-failure",
        temp.path(),
        temp.path(),
    )
    .await;
    let source =
        seed_project_persona(&state, "project-refine-draft-failure-source", &project.id).await;
    let mut seeded = ChatConversation::new_project(project.id.clone());
    seeded.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
    seeded.set_coordination_mode(CoordinationMode::Solo);
    let seeded = state
        .chat_conversation_repo
        .create(seeded)
        .await
        .expect("empty conversation seed should persist");
    db.with_connection(|connection| {
        connection
            .execute_batch(
                "CREATE TRIGGER fail_seeded_refine_draft_insert
                 BEFORE INSERT ON personas
                 BEGIN SELECT RAISE(ABORT, 'forced seeded refine draft failure'); END;",
            )
            .expect("persona insert failure trigger should install");
    });
    let workspace = standalone_workspace_path(
        &standalone_workspaces_root(state.app_paths.app_data_dir()),
        &seeded.id.as_str(),
    );
    let app = build_app(state, Arc::new(ExecutionState::new()));
    let mut input = service_start_input(
        &project.id,
        "Refine without leaking partial start state",
        "persona_builder",
        None,
        None,
        Some(&seeded.id),
        None,
    );
    input.source_persona_id = Some(source.id.as_str().to_string());
    input.provider_harness = Some("codex".to_string());

    let error = start_with_app(&app, input)
        .await
        .expect_err("failed draft creation must reject the start");

    assert!(
        error.contains("forced seeded refine draft failure"),
        "unexpected error: {error}"
    );
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&seeded.id)
        .await
        .expect("conversation lookup should succeed")
        .expect("the pre-existing conversation must remain");
    assert_eq!(
        stored.agent_mode,
        Some(AgentConversationWorkspaceMode::Chat),
        "failed draft creation must restore the prior conversation mode"
    );
    assert_eq!(
        stored.coordination_mode,
        CoordinationMode::Solo,
        "failed draft creation must restore the prior coordination mode"
    );
    assert!(
        stored.builder_draft_id.is_none(),
        "the failed transaction must not leave a draft binding"
    );
    assert!(
        app.state::<AppState>()
            .persona_repo
            .get_draft_by_source_persona_id(&source.id)
            .await
            .expect("seeded draft lookup should succeed")
            .is_none(),
        "the failed transaction must not leave a seeded draft"
    );
    assert!(
        !ralphx_lib::utils::path_safety::checked_exists(
            &workspace,
            "seeded refine failure workspace",
        )
        .expect("workspace path should remain safe"),
        "failed draft creation must remove the newly created private workspace"
    );
    let runs = app
        .state::<AppState>()
        .agent_run_repo
        .get_by_conversation(&seeded.id)
        .await
        .expect("agent run lookup should succeed");
    assert!(
        runs.is_empty(),
        "failed preparation must not create an agent run"
    );
    assert!(
        !fake_codex.was_invoked(),
        "failed preparation must not spawn the selected provider"
    );
}

#[tokio::test]
async fn seeded_refine_start_enforces_source_status_and_exact_scope_then_stamps_provenance() {
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
    let project_a = seed_project(&state, "project-refine-a", temp.path(), temp.path()).await;
    let project_b = seed_project(&state, "project-refine-b", temp.path(), temp.path()).await;
    let global_source = seed_persona(&state, "global-refine-source", PersonaStatus::Active).await;
    let archived_source =
        seed_persona(&state, "archived-refine-source", PersonaStatus::Archived).await;
    let project_source = seed_project_persona(&state, "project-refine-source", &project_a.id).await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let mut missing = service_start_input(
        &project_a.id,
        "Missing source",
        "persona_builder",
        None,
        None,
        None,
        None,
    );
    missing.source_persona_id = Some("missing-source".to_string());
    assert!(start_with_app(&app, missing)
        .await
        .expect_err("missing source must reject")
        .contains("not found"));

    let mut archived =
        standalone_start_input("Archived source", Some("persona_builder"), None, None, None);
    archived.source_persona_id = Some(archived_source.id.as_str().to_string());
    assert!(start_with_app(&app, archived)
        .await
        .expect_err("archived source must reject")
        .contains("not active"));

    let mut global_in_project = service_start_input(
        &project_a.id,
        "Wrong global scope",
        "persona_builder",
        None,
        None,
        None,
        None,
    );
    global_in_project.source_persona_id = Some(global_source.id.as_str().to_string());
    assert!(start_with_app(&app, global_in_project)
        .await
        .expect_err("global source cannot refine in Project context")
        .contains("PERSONA_REFINE_SCOPE_MISMATCH"));

    let mut project_in_global = standalone_start_input(
        "Wrong project scope",
        Some("persona_builder"),
        None,
        None,
        None,
    );
    project_in_global.source_persona_id = Some(project_source.id.as_str().to_string());
    assert!(start_with_app(&app, project_in_global)
        .await
        .expect_err("project source cannot refine in Standalone context")
        .contains("PERSONA_REFINE_SCOPE_MISMATCH"));

    let mut project_a_in_b = service_start_input(
        &project_b.id,
        "Wrong project identity",
        "persona_builder",
        None,
        None,
        None,
        None,
    );
    project_a_in_b.source_persona_id = Some(project_source.id.as_str().to_string());
    assert!(start_with_app(&app, project_a_in_b)
        .await
        .expect_err("project-A source cannot refine in project-B context")
        .contains("PERSONA_REFINE_SCOPE_MISMATCH"));

    let mut matching_project = service_start_input(
        &project_a.id,
        "Matching project refine",
        "persona_builder",
        None,
        None,
        None,
        None,
    );
    matching_project.source_persona_id = Some(project_source.id.as_str().to_string());
    let project_started = start_with_app(&app, matching_project)
        .await
        .expect("matching project scope should seed");
    let project_draft = app
        .state::<AppState>()
        .persona_repo
        .get_by_id(&PersonaId::from(
            project_started
                .conversation
                .builder_draft_id
                .as_deref()
                .expect("seeded project draft must be bound"),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(project_draft.project_id.as_ref(), Some(&project_a.id));
    assert_eq!(
        project_draft.source_persona_id.as_ref(),
        Some(&project_source.id)
    );
    assert_eq!(
        project_draft.source_content_hash.as_deref(),
        Some(project_source.content_hash.as_str())
    );

    let mut matching_global = standalone_start_input(
        "Matching global refine",
        Some("persona_builder"),
        None,
        None,
        None,
    );
    matching_global.source_persona_id = Some(global_source.id.as_str().to_string());
    let global_started = start_with_app(&app, matching_global)
        .await
        .expect("matching global scope should seed");
    let global_draft = app
        .state::<AppState>()
        .persona_repo
        .get_by_id(&PersonaId::from(
            global_started
                .conversation
                .builder_draft_id
                .as_deref()
                .expect("seeded global draft must be bound"),
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(
        global_draft.project_id.is_none(),
        "Standalone seeded draft must remain global"
    );
    assert_eq!(
        global_draft.source_persona_id.as_ref(),
        Some(&global_source.id)
    );
    assert_eq!(
        global_draft.source_content_hash.as_deref(),
        Some(global_source.content_hash.as_str())
    );
}

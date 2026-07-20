use super::agent_conversation_start_support::*;

#[tokio::test]
async fn start_with_persona_persists_binding_and_first_send_includes_persona_block() {
    let _persona_feature = enable_personas_for_test();
    let _allow_spawn =
        super::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(&state, "project-start-persona", temp.path(), temp.path()).await;
    let persona = seed_persona(&state, "start-persona", PersonaStatus::Active).await;
    let app = build_app(state, Arc::new(ExecutionState::new()));
    let mut input = service_start_input(
        &project.id,
        "Start with a persona",
        "chat",
        None,
        None,
        None,
        None,
    );
    input.persona_id = Some(persona.id.as_str().to_string());
    input.provider_harness = Some("claude".to_string());

    let started = start_with_app(&app, input)
        .await
        .expect("persona-bound conversation should start");
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&started.conversation.id)
        .await
        .expect("conversation lookup should succeed")
        .expect("conversation should persist");
    assert_eq!(stored.persona_id.as_deref(), Some(persona.id.as_str()));

    assert!(
        fake_cli
            .captured_prompt()
            .await
            .contains("<ralphx_agent_persona>"),
        "the start-path override must not suppress the explicit first-send persona"
    );
}

#[tokio::test]
async fn start_input_persona_id_rejected_for_non_project_context() {
    let _persona_feature = enable_personas_for_test();
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-persona-non-project",
        temp.path(),
        temp.path(),
    )
    .await;
    let persona = seed_persona(&state, "non-project-persona", PersonaStatus::Active).await;
    let task_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_task(TaskId::from_string(
            "task-start-persona-non-project".to_string(),
        )))
        .await
        .expect("task conversation fixture should persist");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);
    let mut input = service_start_input(
        &project.id,
        "Do not bind on a task conversation",
        "chat",
        None,
        None,
        Some(&task_conversation.id),
        None,
    );
    input.persona_id = Some(persona.id.as_str().to_string());

    let error = start_with_app(&app, input)
        .await
        .expect_err("persona input must reject non-Project conversations");
    assert!(
        error.contains("Persona bindings require Project conversation context"),
        "unexpected typed context rejection: {error}"
    );
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&task_conversation.id)
        .await
        .expect("task conversation lookup should succeed")
        .expect("task conversation should remain");
    assert!(stored.persona_id.is_none());
}

#[tokio::test]
async fn start_with_persona_flag_off_fails_before_creating_conversation_or_workspace() {
    let _persona_feature =
        super::support::env::EnvVarGuard::set("RALPHX_UI_AGENT_PERSONAS", "false");
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-persona-flag-off",
        temp.path(),
        temp.path(),
    )
    .await;
    let persona = seed_persona(&state, "feature-off-persona", PersonaStatus::Active).await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);
    let mut input = service_start_input(
        &project.id,
        "Do not create an edit workspace when personas are disabled",
        "edit",
        None,
        None,
        None,
        None,
    );
    input.persona_id = Some(persona.id.as_str().to_string());

    let error = start_with_app(&app, input)
        .await
        .expect_err("persona feature flag must be enforced before setup side effects");

    assert!(
        error.contains("[Personas disabled:"),
        "unexpected error: {error}"
    );
    assert!(app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_context(ChatContextType::Project, project.id.as_str())
        .await
        .expect("conversation lookup should succeed")
        .is_empty());
    assert!(app
        .state::<AppState>()
        .agent_conversation_workspace_repo
        .get_by_project_id(&project.id)
        .await
        .expect("workspace lookup should succeed")
        .is_empty());
}

#[tokio::test]
async fn start_with_draft_or_archived_persona_fails_closed_without_binding() {
    let _persona_feature = enable_personas_for_test();
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-persona-inactive",
        temp.path(),
        temp.path(),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    for status in [PersonaStatus::Draft, PersonaStatus::Archived] {
        let persona = seed_persona(
            app.state::<AppState>().inner(),
            &format!("inactive-start-persona-{status}"),
            status,
        )
        .await;
        let conversation = app
            .state::<AppState>()
            .chat_conversation_repo
            .create(ChatConversation::new_project(project.id.clone()))
            .await
            .expect("seeded conversation should persist");
        let mut input = service_start_input(
            &project.id,
            "Reject inactive persona",
            "chat",
            None,
            None,
            Some(&conversation.id),
            None,
        );
        input.persona_id = Some(persona.id.as_str().to_string());

        let error = start_with_app(&app, input)
            .await
            .expect_err("draft and archived personas must fail closed");
        assert!(
            error.contains("[Persona unavailable:"),
            "unexpected inactive persona error: {error}"
        );
        let stored = app
            .state::<AppState>()
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .expect("conversation lookup should succeed")
            .expect("conversation should remain");
        assert!(stored.persona_id.is_none());
    }
}

#[tokio::test]
async fn start_with_cross_project_persona_fails_closed_without_binding() {
    let _persona_feature = enable_personas_for_test();
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(&state, "project-start-scope-a", temp.path(), temp.path()).await;
    let other_project_id = ProjectId::from_string("project-start-scope-b".to_string());
    let persona = seed_project_persona(&state, "cross-project-persona", &other_project_id).await;
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);
    let mut input = service_start_input(
        &project.id,
        "Reject cross-project persona",
        "chat",
        None,
        None,
        Some(&conversation.id),
        None,
    );
    input.persona_id = Some(persona.id.to_string());

    let error = start_with_app(&app, input)
        .await
        .expect_err("cross-project persona must fail before binding");
    assert!(error.contains("[Persona unavailable:"));
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert!(stored.persona_id.is_none());
}

#[tokio::test]
async fn start_without_persona_id_unchanged() {
    let _persona_feature = enable_personas_for_test();
    let _allow_spawn =
        super::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let fake_cli = CapturingFakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_cli.cli_path.to_str().expect("utf8 fake CLI path"),
    );
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-without-persona",
        temp.path(),
        temp.path(),
    )
    .await;
    let app = build_app(state, Arc::new(ExecutionState::new()));

    let mut input = service_start_input(
        &project.id,
        "Start without a persona",
        "chat",
        None,
        None,
        None,
        None,
    );
    input.provider_harness = Some("claude".to_string());
    let started = start_with_app(&app, input)
        .await
        .expect("persona-free conversation should start");
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&started.conversation.id)
        .await
        .expect("conversation lookup should succeed")
        .expect("conversation should persist");
    assert!(stored.persona_id.is_none());
    assert!(
        !fake_cli
            .captured_prompt()
            .await
            .contains("<ralphx_agent_persona>"),
        "persona-free starts must keep the prior prompt shape"
    );
}

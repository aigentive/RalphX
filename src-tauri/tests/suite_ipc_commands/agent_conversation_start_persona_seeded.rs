use super::agent_conversation_start_support::*;

#[tokio::test]
async fn start_agent_conversation_persona_builder_rejects_project_team_intent() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    let app = build_app(AppState::new_test(), Arc::new(ExecutionState::new()));
    let project_id = ProjectId::from_string("project-builder-team-rejected".to_string());
    let mut input = service_start_input(
        &project_id,
        "Team builder is undefined",
        "persona_builder",
        None,
        None,
        None,
        None,
    );
    input.team_intent = Some(TeamIntent::rx_native(None));

    let error = start_with_app(&app, input)
        .await
        .expect_err("Project-context builder Team intent must be rejected");
    assert!(
        error.contains("persona builder"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn seeded_project_persona_builder_rejects_persisted_team_coordination() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-builder-persisted-team",
        temp.path(),
        temp.path(),
    )
    .await;
    let mut seeded = ChatConversation::new_project(project.id.clone());
    seeded.set_agent_mode(Some(AgentConversationWorkspaceMode::PersonaBuilder));
    seeded.set_coordination_mode(CoordinationMode::RxNativeTeam);
    let seeded = state
        .chat_conversation_repo
        .create(seeded)
        .await
        .expect("corrupt Project builder seed should persist for the regression");
    let app = build_app(state, Arc::new(ExecutionState::new()));

    let error = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Reject persisted Team builder",
            "persona_builder",
            None,
            None,
            Some(&seeded.id),
            None,
        ),
    )
    .await
    .expect_err("seeded Project builder with Team coordination must reject");
    assert!(
        error.contains("persona builder"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn seeded_persona_builder_rejects_chat_mode_as_locked() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-builder-mode-lock".to_string());
    let mut seeded = ChatConversation::new_project(project_id.clone());
    seeded.set_agent_mode(Some(AgentConversationWorkspaceMode::PersonaBuilder));
    let seeded = state
        .chat_conversation_repo
        .create(seeded)
        .await
        .expect("seeded builder should persist");
    let app = build_app(state, Arc::new(ExecutionState::new()));

    let error = start_with_app(
        &app,
        service_start_input(
            &project_id,
            "Do not rewrite the persisted builder mode",
            "chat",
            None,
            None,
            Some(&seeded.id),
            None,
        ),
    )
    .await
    .expect_err("seeded builder mode must be locked");

    assert!(error.contains("[ralphx:conversation_mode_locked]"));
    let mut omitted_mode = service_start_input(
        &project_id,
        "Omitted mode must not rewrite the persisted builder mode",
        "chat",
        None,
        None,
        Some(&seeded.id),
        None,
    );
    omitted_mode.mode = None;
    let omitted_error = start_with_app(&app, omitted_mode)
        .await
        .expect_err("omitted seeded builder mode must be locked");
    assert!(omitted_error.contains("[ralphx:conversation_mode_locked]"));
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&seeded.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.agent_mode,
        Some(AgentConversationWorkspaceMode::PersonaBuilder)
    );
}

#[tokio::test]
async fn seeded_non_builder_with_messages_rejects_conversion_to_persona_builder() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    let state = AppState::new_test();
    let project_id =
        ProjectId::from_string("project-message-bearing-builder-conversion".to_string());
    let seeded = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("seed ordinary conversation");
    state
        .chat_message_repo
        .create(ChatMessage {
            id: ChatMessageId::new(),
            session_id: None,
            project_id: Some(project_id.clone()),
            task_id: None,
            conversation_id: Some(seeded.id),
            role: MessageRole::User,
            content: "already started as chat".to_string(),
            metadata: None,
            parent_message_id: None,
            tool_calls: None,
            content_blocks: None,
            attribution_source: None,
            provider_harness: None,
            provider_session_id: None,
            upstream_provider: None,
            provider_profile: None,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
            input_tokens: None,
            output_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            estimated_usd: None,
            usage_provenance: None,
            raw_usage_snapshot: None,
            created_at: Utc::now(),
        })
        .await
        .expect("seed prior chat message");
    let app = build_app(state, Arc::new(ExecutionState::new()));

    let error = start_with_app(
        &app,
        service_start_input(
            &project_id,
            "Do not convert prior chat history",
            "persona_builder",
            None,
            None,
            Some(&seeded.id),
            None,
        ),
    )
    .await
    .expect_err("message-bearing non-builder must not convert to builder");

    assert!(error.contains("[ralphx:conversation_mode_locked]"));
}

#[tokio::test]
async fn fresh_empty_non_builder_seed_remains_convertible_to_persona_builder() {
    let _reset = PersonaFlagsOverrideReset;
    set_agent_personas_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-empty-builder-conversion",
        temp.path(),
        temp.path(),
    )
    .await;
    let seeded = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("seed empty ordinary conversation");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let started = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Convert the unused seed",
            "persona_builder",
            None,
            None,
            Some(&seeded.id),
            None,
        ),
    )
    .await
    .expect("empty non-builder seed remains convertible");

    assert!(started.send_result.was_queued);
    assert_eq!(
        started.conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::PersonaBuilder)
    );
}

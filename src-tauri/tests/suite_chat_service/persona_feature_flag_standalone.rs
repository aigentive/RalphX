use super::persona_feature_flag_support::*;

#[tokio::test]
async fn standalone_chat_fresh_send_rejects_codex_override() {
    let _persona_reset = PersonaFlagOverrideReset;
    let _standalone_reset = StandaloneFlagOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(true));
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("standalone builder send temp directory");
    let mut initial_state = AppState::new_test();
    initial_state.app_paths = AppPaths::new(temp.path().join("app-data"), None);
    let conversation = initial_state
        .chat_conversation_repo
        .create({
            let mut conversation = ChatConversation::new_standalone();
            conversation.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
            conversation
        })
        .await
        .expect("create standalone builder conversation");
    create_workspace(
        initial_state.app_paths.app_data_dir(),
        &conversation.id.as_str(),
    )
    .expect("create standalone builder workspace");
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build standalone builder app");
    let service = app
        .state::<AppState>()
        .build_chat_service_for_runtime::<tauri::test::MockRuntime>(
            None,
            Some(app.handle().clone()),
        )
        .with_persona_feature_enabled(true)
        .with_working_directory(temp.path());
    let context_id = conversation.id.as_str();

    let error = service
        .send_message(
            ChatContextType::Standalone,
            &context_id,
            "Do not start this standalone chat on Codex.",
            SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                harness_override: Some(AgentHarnessKind::Codex),
                ..Default::default()
            },
        )
        .await
        .expect_err("standalone chat Codex send must reject");

    assert!(matches!(
        error,
        ChatServiceError::SpawnFailed(ref message)
            if message == STANDALONE_CODEX_UNSUPPORTED_ERROR
    ));
}

#[test]
fn codex_send_guard_allows_project_chat_and_rejects_standalone_chat() {
    let mut project_chat = ChatConversation::new_project(
        ralphx_lib::domain::entities::ProjectId::from_string("project-chat-codex".to_string()),
    );
    project_chat.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    validate_conversation_spawn_harness(&project_chat, AgentHarnessKind::Codex)
        .expect("Project-context chat must still allow Codex sends");

    let mut standalone_chat = ChatConversation::new_standalone();
    standalone_chat.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    let error = validate_conversation_spawn_harness(&standalone_chat, AgentHarnessKind::Codex)
        .expect_err("Standalone chat must reject Codex sends");
    assert!(matches!(
        error,
        ChatServiceError::SpawnFailed(ref message) if message == STANDALONE_CODEX_UNSUPPORTED_ERROR
    ));
}

#[tokio::test]
async fn standalone_chat_queue_rejects_codex_override_with_agent_error() {
    let _standalone_reset = StandaloneFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let mut conversation = ChatConversation::new_standalone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    let conversation = conversation_repo
        .create(conversation)
        .await
        .expect("create queued standalone builder conversation");
    let mut initial_state = AppState::new_test();
    initial_state.chat_conversation_repo = conversation_repo;
    let context_id = conversation.id.as_str();
    initial_state
        .message_queue
        .queue_with_runtime_overrides_and_project_references(
            ChatContextType::Standalone,
            &context_id,
            "queued unsafe provider switch".to_string(),
            None,
            None,
            Some(AgentHarnessKind::Codex),
            None,
            ralphx_lib::domain::entities::PersonaDirective::Inherit,
            None,
            None,
            None,
            false,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            Vec::new(),
        );
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build queued standalone builder app");
    let errors = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&errors);
    let _listener = app.listen("agent:error", move |event| {
        captured.lock().unwrap().push(event.payload().to_string());
    });

    let (processed, last_run_id) = process_queued_messages_for_test(
        app.handle().clone(),
        ChatContextType::Standalone,
        AgentHarnessKind::Claude,
        &context_id,
        conversation.id,
        "claude-session",
        std::path::Path::new("/definitely/missing/ralphx-test-cli"),
    )
    .await;

    assert_eq!(processed, 1);
    assert!(last_run_id.is_none());
    assert!(errors
        .lock()
        .unwrap()
        .iter()
        .any(|payload| payload.contains(STANDALONE_CODEX_UNSUPPORTED_ERROR)));
    assert!(app
        .state::<AppState>()
        .agent_run_repo
        .get_by_conversation(&conversation.id)
        .await
        .unwrap()
        .is_empty());
    assert!(app
        .state::<AppState>()
        .message_queue
        .get_queued(ChatContextType::Standalone, &context_id)
        .is_empty());
}

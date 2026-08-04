use super::persona_feature_flag_support::*;

#[tokio::test]
async fn standalone_chat_fresh_send_accepts_codex_override() {
    let _persona_reset = PersonaFlagOverrideReset;
    let _standalone_reset = StandaloneFlagOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(true));
    let fake_codex = FakeCodex::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_codex
            .cli_path
            .to_str()
            .expect("fake Codex path should be UTF-8"),
    );
    let temp = tempfile::tempdir().expect("standalone Codex send temp directory");
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
        .expect("create standalone Codex conversation");
    create_workspace(
        initial_state.app_paths.app_data_dir(),
        &conversation.id.as_str(),
    )
    .expect("create standalone Codex workspace");
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build standalone Codex app");
    let service = app
        .state::<AppState>()
        .build_chat_service()
        .with_persona_feature_enabled(true)
        .with_working_directory(temp.path());
    let context_id = conversation.id.as_str();

    let result = service
        .send_message(
            ChatContextType::Standalone,
            &context_id,
            "Start this standalone chat on Codex.",
            SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                harness_override: Some(AgentHarnessKind::Codex),
                ..Default::default()
            },
        )
        .await
        .expect("standalone chat Codex send should be accepted");

    assert!(!result.was_queued);
    let runs = app
        .state::<AppState>()
        .agent_run_repo
        .get_by_conversation(&conversation.id)
        .await
        .expect("standalone Codex run lookup should succeed");
    let run = runs
        .iter()
        .find(|run| {
            run.id.as_str() == result.agent_run_id && run.harness == Some(AgentHarnessKind::Codex)
        })
        .expect("standalone Codex run should persist provider attribution");
    assert_eq!(run.approval_policy.as_deref(), Some("on-request"));
    assert_eq!(run.sandbox_mode.as_deref(), Some("workspace-write"));
    assert!(
        fake_codex.wait_until_invoked().await,
        "fresh Standalone Chat send must invoke Codex"
    );
}

#[tokio::test]
async fn standalone_chat_fresh_send_accepts_claude_override() {
    let _persona_reset = PersonaFlagOverrideReset;
    let _standalone_reset = StandaloneFlagOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(true));
    let fake_claude = FakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_claude
            .cli_path
            .to_str()
            .expect("fake Claude path should be UTF-8"),
    );
    let temp = tempfile::tempdir().expect("standalone Claude send temp directory");
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
        .expect("create standalone Claude conversation");
    create_workspace(
        initial_state.app_paths.app_data_dir(),
        &conversation.id.as_str(),
    )
    .expect("create standalone Claude workspace");
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build standalone Claude app");
    let service = app
        .state::<AppState>()
        .build_chat_service()
        .with_persona_feature_enabled(true)
        .with_working_directory(temp.path());
    let context_id = conversation.id.as_str();

    let result = service
        .send_message(
            ChatContextType::Standalone,
            &context_id,
            "Start this standalone chat on Claude.",
            SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                harness_override: Some(AgentHarnessKind::Claude),
                ..Default::default()
            },
        )
        .await
        .expect("standalone chat Claude send should be accepted");

    assert!(!result.was_queued);
    let runs = app
        .state::<AppState>()
        .agent_run_repo
        .get_by_conversation(&conversation.id)
        .await
        .expect("standalone Claude run lookup should succeed");
    assert!(runs.iter().any(|run| {
        run.id.as_str() == result.agent_run_id && run.harness == Some(AgentHarnessKind::Claude)
    }));
    assert!(
        fake_claude.wait_until_invoked().await,
        "fresh Standalone Chat send must invoke Claude"
    );
}

#[tokio::test]
async fn standalone_chat_queue_accepts_codex_override() {
    let _standalone_reset = StandaloneFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let provider_session_id = "standalone-codex-session";
    let provider_state = tempfile::tempdir().expect("queued Codex provider state");
    let session_index = provider_state.path().join(".codex/session_index.jsonl");
    validate_absolute_non_root_path(provider_state.path(), "queued Codex provider state root")
        .expect("queued Codex provider state root should be contained");
    validate_absolute_non_root_path(&session_index, "queued Codex session index")
        .expect("queued Codex session index should be contained");
    fs::create_dir_all(session_index.parent().expect("Codex state parent"))
        .expect("create queued Codex provider state");
    fs::write(
        &session_index,
        format!(r#"{{"id":"{provider_session_id}"}}"#),
    )
    .expect("seed queued Codex session artifact");
    let _provider_state = PersonaEnvReset::set(
        "RALPHX_PROVIDER_STATE_HOME_OVERRIDE",
        provider_state
            .path()
            .to_str()
            .expect("provider state UTF-8"),
    );
    let fake_codex = FakeCodex::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_codex
            .cli_path
            .to_str()
            .expect("fake Codex path should be UTF-8"),
    );
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let mut conversation = ChatConversation::new_standalone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    let conversation = conversation_repo
        .create(conversation)
        .await
        .expect("create queued standalone Codex conversation");
    let temp = tempfile::tempdir().expect("queued standalone Codex app data");
    let mut initial_state = AppState::new_test();
    initial_state.chat_conversation_repo = conversation_repo;
    initial_state.app_paths = AppPaths::new(temp.path().join("app-data"), None);
    let events = RecordingEventSink::new();
    initial_state.events = Arc::new(events.clone());
    let context_id = conversation.id.as_str();
    create_workspace(initial_state.app_paths.app_data_dir(), &context_id)
        .expect("create queued standalone private workspace");
    initial_state
        .message_queue
        .queue_with_runtime_overrides_and_project_references(
            ChatContextType::Standalone,
            &context_id,
            "queued standalone Codex message".to_string(),
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
            Vec::new(),
        );
    seed_completed_continuation_runtime(
        &initial_state,
        &conversation.id,
        AgentHarnessKind::Codex,
        provider_session_id,
    )
    .await;
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build queued standalone Codex app");
    let state = app.state::<AppState>();
    let (processed, last_run_id) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Standalone,
        AgentHarnessKind::Codex,
        &context_id,
        conversation.id,
        provider_session_id,
        &fake_codex.cli_path,
    )
    .await;

    assert_eq!(processed, 1);
    let last_run_id = last_run_id.expect("queued standalone Codex run should start");
    assert!(recorded_event_payloads(&events, "agent:error").is_empty());
    let runs = app
        .state::<AppState>()
        .agent_run_repo
        .get_by_conversation(&conversation.id)
        .await
        .expect("queued standalone Codex run lookup should succeed");
    let run = runs
        .iter()
        .find(|run| run.id.as_str() == last_run_id && run.harness == Some(AgentHarnessKind::Codex))
        .expect("queued standalone Codex continuation should persist provider attribution");
    assert_eq!(run.approval_policy.as_deref(), Some("on-request"));
    assert_eq!(run.sandbox_mode.as_deref(), Some("workspace-write"));
    assert!(app
        .state::<AppState>()
        .message_queue
        .get_queued(ChatContextType::Standalone, &context_id)
        .is_empty());
    assert!(
        fake_codex.wait_until_invoked().await,
        "same-provider queue continuation must invoke Codex"
    );
    let invocation_args = fake_codex.invocation_args();
    assert!(
        invocation_args.lines().any(|argument| argument == "resume"),
        "queued Codex continuation must use the native resume subcommand: {invocation_args}"
    );
}

#[tokio::test]
async fn standalone_chat_queue_accepts_claude_override() {
    let _standalone_reset = StandaloneFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let provider_session_id = "standalone-claude-session";
    let fake_claude = FakeClaude::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_claude
            .cli_path
            .to_str()
            .expect("fake Claude path should be UTF-8"),
    );
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let mut conversation = ChatConversation::new_standalone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    let conversation = conversation_repo
        .create(conversation)
        .await
        .expect("create queued standalone Claude conversation");
    let temp = tempfile::tempdir().expect("queued standalone Claude app data");
    let mut initial_state = AppState::new_test();
    initial_state.chat_conversation_repo = conversation_repo;
    initial_state.app_paths = AppPaths::new(temp.path().join("app-data"), None);
    let events = RecordingEventSink::new();
    initial_state.events = Arc::new(events.clone());
    let context_id = conversation.id.as_str();
    create_workspace(initial_state.app_paths.app_data_dir(), &context_id)
        .expect("create queued standalone private workspace");
    initial_state
        .message_queue
        .queue_with_runtime_overrides_and_project_references(
            ChatContextType::Standalone,
            &context_id,
            "queued standalone Claude message".to_string(),
            None,
            None,
            Some(AgentHarnessKind::Claude),
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
            Vec::new(),
        );
    seed_completed_continuation_runtime(
        &initial_state,
        &conversation.id,
        AgentHarnessKind::Claude,
        provider_session_id,
    )
    .await;
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build queued standalone Claude app");
    let state = app.state::<AppState>();
    let (processed, last_run_id) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Standalone,
        AgentHarnessKind::Claude,
        &context_id,
        conversation.id,
        provider_session_id,
        &fake_claude.cli_path,
    )
    .await;

    assert_eq!(processed, 1);
    let last_run_id = last_run_id.expect("queued standalone Claude run should start");
    assert!(recorded_event_payloads(&events, "agent:error").is_empty());
    let runs = app
        .state::<AppState>()
        .agent_run_repo
        .get_by_conversation(&conversation.id)
        .await
        .expect("queued standalone Claude run lookup should succeed");
    assert!(runs.iter().any(|run| {
        run.id.as_str() == last_run_id && run.harness == Some(AgentHarnessKind::Claude)
    }));
    assert!(app
        .state::<AppState>()
        .message_queue
        .get_queued(ChatContextType::Standalone, &context_id)
        .is_empty());
    assert!(
        fake_claude.wait_until_invoked().await,
        "matching queue identity must invoke Claude"
    );
}

#[tokio::test]
async fn standalone_chat_queue_rejects_caller_context_downgrade_without_spawning() {
    let _standalone_reset = StandaloneFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let fake_provider = FakeCodex::new();
    ralphx_lib::testing::seed_available_harness_probes_for_test_at(
        fake_provider
            .cli_path
            .to_str()
            .expect("fake provider path should be UTF-8"),
    );
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let mut conversation = ChatConversation::new_standalone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    let conversation = conversation_repo
        .create(conversation)
        .await
        .expect("create persisted standalone conversation");
    let mut initial_state = AppState::new_test();
    initial_state.chat_conversation_repo = conversation_repo;
    let events = RecordingEventSink::new();
    initial_state.events = Arc::new(events.clone());
    let context_id = conversation.context_id.as_str();
    initial_state
        .message_queue
        .queue_with_runtime_overrides_and_project_references(
            ChatContextType::Project,
            context_id,
            "attempt a downgraded queued launch".to_string(),
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
            Vec::new(),
        );
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build mismatched queue app");
    let state = app.state::<AppState>();
    let (processed, last_run_id) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        context_id,
        conversation.id,
        "claude-session",
        &fake_provider.cli_path,
    )
    .await;

    assert_eq!(processed, 1);
    assert_eq!(
        last_run_id, None,
        "mismatched identity must not start a run"
    );
    assert!(
        recorded_event_payloads(&events, "agent:error")
            .iter()
            .any(|error| error.to_string().contains("context type mismatch")),
        "identity mismatch must surface as an agent error",
    );
    assert!(
        app.state::<AppState>()
            .agent_run_repo
            .get_by_conversation(&conversation.id)
            .await
            .expect("run lookup should succeed")
            .is_empty(),
        "mismatched identity must not persist a provider run",
    );
    assert!(
        !fake_provider.was_invoked(),
        "mismatched persisted Standalone identity must not invoke the selected provider",
    );
}

#[tokio::test]
async fn standalone_chat_queue_missing_authoritative_conversation_fails_without_spawning() {
    let fake_provider = FakeCodex::new();
    let conversation_id = ChatConversationId::new();
    let context_id = conversation_id.as_str();
    let temp = tempfile::tempdir().expect("orphaned queue workspace temp directory");
    let mut initial_state = AppState::new_test();
    initial_state.app_paths = AppPaths::new(temp.path().join("app-data"), None);
    create_workspace(initial_state.app_paths.app_data_dir(), &context_id)
        .expect("create orphaned queue workspace fixture");
    initial_state.message_queue.queue(
        ChatContextType::Standalone,
        &context_id,
        "orphaned standalone queue entry".to_string(),
    );
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build orphaned queue app");

    let state = app.state::<AppState>();
    let (processed, last_run_id) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Standalone,
        AgentHarnessKind::Codex,
        &context_id,
        conversation_id,
        "missing-provider-session",
        &fake_provider.cli_path,
    )
    .await;

    assert_eq!(processed, 1);
    assert_eq!(last_run_id, None);
    assert!(
        app.state::<AppState>()
            .agent_run_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("run lookup should succeed")
            .is_empty(),
        "missing authoritative conversation must not persist a run",
    );
    assert!(
        !fake_provider.was_invoked(),
        "missing authoritative conversation must not invoke the selected provider",
    );
}

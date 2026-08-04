use super::persona_feature_flag_support::*;

#[tokio::test]
async fn resumed_builder_send_rejects_flag_off_and_flag_on_passes_the_gate() {
    let state = AppState::new_test();
    let project = Project::new("Resumed builder flag gate".to_string(), ".".to_string());
    state.project_repo.create(project.clone()).await.unwrap();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed resumed builder");
    let options = SendMessageOptions {
        conversation_id_override: Some(conversation.id),
        ..Default::default()
    };

    let disabled = persona_flag_override_chat_service(&state)
        .with_persona_feature_enabled(false)
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Resume while disabled",
            options.clone(),
        )
        .await
        .expect_err("disabled builder resume must reject");
    assert!(matches!(
        disabled,
        ChatServiceError::PersonaUnavailable(ref message)
            if message == PERSONA_BUILDER_FEATURE_DISABLED_ERROR
    ));
    assert!(state
        .agent_run_repo
        .get_by_conversation(&conversation.id)
        .await
        .unwrap()
        .is_empty());

    let enabled = persona_flag_override_chat_service(&state)
        .with_persona_feature_enabled(true)
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Resume while enabled",
            options,
        )
        .await;
    assert!(
        !matches!(
            enabled,
            Err(ChatServiceError::PersonaUnavailable(ref message))
                if message == PERSONA_BUILDER_FEATURE_DISABLED_ERROR
        ),
        "flag-on builder resume must pass the feature gate: {enabled:?}"
    );
}

#[tokio::test]
async fn queued_builder_drain_rejects_flag_off_and_flag_on_passes_the_gate() {
    async fn drain_with_flag(enabled: bool) -> Vec<String> {
        let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
        let project_id = ralphx_lib::domain::entities::ProjectId::from_string(format!(
            "queued-builder-flag-{enabled}"
        ));
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
        let conversation = conversation_repo.create(conversation).await.unwrap();
        let mut initial_state = AppState::new_test();
        initial_state.chat_conversation_repo = conversation_repo;
        let queue_context_id = conversation.id.as_str();
        initial_state
            .message_queue
            .queue_with_runtime_overrides_and_project_references(
                ChatContextType::Project,
                &queue_context_id,
                "queued builder resume".to_string(),
                None,
                None,
                None,
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
            .unwrap();
        let errors = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&errors);
        let _listener = app.listen("agent:error", move |event| {
            captured.lock().unwrap().push(event.payload().to_string());
        });
        let state = app.state::<AppState>();
        process_queued_messages_for_test_with_persona_feature(
            state.inner(),
            None,
            Arc::clone(&state.events),
            ChatContextType::Project,
            AgentHarnessKind::Claude,
            project_id.as_str(),
            conversation.id,
            "claude-session",
            std::path::Path::new("/definitely/missing/ralphx-test-cli"),
            enabled,
        )
        .await;
        let captured_errors = errors.lock().unwrap().clone();
        captured_errors
    }

    let disabled_errors = drain_with_flag(false).await;
    assert!(disabled_errors
        .iter()
        .any(|payload| payload.contains(PERSONA_BUILDER_FEATURE_DISABLED_ERROR)));

    let enabled_errors = drain_with_flag(true).await;
    assert!(enabled_errors
        .iter()
        .all(|payload| !payload.contains(PERSONA_BUILDER_FEATURE_DISABLED_ERROR)));
}

#[tokio::test]
async fn queued_builder_conversation_lookup_error_surfaces_without_spawn() {
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let project = Project::new("Queue lookup failure".to_string(), ".".to_string());
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = conversation_repo
        .create(conversation)
        .await
        .expect("create queued builder conversation");
    conversation_repo.fail_get_by_id(conversation.id).await;
    let mut initial_state = AppState::new_test();
    initial_state.chat_conversation_repo = conversation_repo;
    let queue_context_id = conversation.id.as_str();
    initial_state
        .message_queue
        .queue_with_runtime_overrides_and_project_references(
            ChatContextType::Project,
            &queue_context_id,
            "queued builder after repository failure".to_string(),
            None,
            None,
            None,
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
        .expect("build queued lookup failure app");
    let errors = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&errors);
    let _listener = app.listen("agent:error", move |event| {
        captured.lock().unwrap().push(event.payload().to_string());
    });

    let state = app.state::<AppState>();
    let (processed, last_run_id) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project.id.as_str(),
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
        .any(|payload| payload.contains("injected conversation lookup failure")));
    assert!(app
        .state::<AppState>()
        .agent_run_repo
        .get_by_conversation(&conversation.id)
        .await
        .unwrap()
        .is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn queued_builder_drain_spawn_args_enable_filesystem_enforcement() {
    use std::os::unix::fs::PermissionsExt;

    let _spawn_permission = PersonaEnvReset::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("queued builder enforcement fixture");
    let capture_path = validate_absolute_non_root_path(
        &temp.path().join("queued-builder-spawn.txt"),
        "queued builder capture",
    )
    .expect("safe queued builder capture");
    let _capture = PersonaEnvReset::set(
        "RALPHX_QUEUE_ARGS_CAPTURE",
        capture_path.to_str().expect("utf8 capture path"),
    );
    let cli_path =
        validate_absolute_non_root_path(&temp.path().join("fake-claude"), "queued builder CLI")
            .expect("safe queued builder CLI");
    fs::write(
        &cli_path,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$RALPHX_QUEUE_ARGS_CAPTURE"
for arg in "$@"; do
  [ -f "$arg" ] && cat "$arg" >> "$RALPHX_QUEUE_ARGS_CAPTURE"
done
cat >/dev/null
printf '%s\n' '{"type":"result","session_id":"queued-builder-session","is_error":false,"result":"ok","cost_usd":0.0}'
"#,
    )
    .expect("write queued builder capture CLI");
    let mut permissions = fs::metadata(&cli_path)
        .expect("queued builder capture CLI metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&cli_path, permissions)
        .expect("mark queued builder capture CLI executable");

    let initial_state = AppState::new_test();
    let project = initial_state
        .project_repo
        .create(Project::new(
            "Queued Builder Enforcement".to_string(),
            temp.path().to_string_lossy().into_owned(),
        ))
        .await
        .expect("persist queued builder project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = initial_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist queued builder conversation");
    seed_completed_continuation_runtime(
        &initial_state,
        &conversation.id,
        AgentHarnessKind::Claude,
        "queued-builder-old-session",
    )
    .await;
    let queue_context_id = conversation.id.as_str();
    initial_state.message_queue.queue(
        ChatContextType::Project,
        queue_context_id.clone(),
        "drain queued builder with enforcement".to_string(),
    );
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build queued builder enforcement app");

    let state = app.state::<AppState>();
    let (processed, _) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project.id.as_str(),
        conversation.id,
        "queued-builder-old-session",
        &cli_path,
    )
    .await;

    assert_eq!(processed, 1);
    let capture_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while tokio::time::Instant::now() < capture_deadline {
        if capture_path.is_file() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let captured = fs::read_to_string(capture_path)
        .expect("read queued builder spawn arguments and MCP config");
    assert!(
        captured.contains("--filesystem-enforced") && captured.contains("\"1\""),
        "queued PersonaBuilder drain must pass filesystem enforcement to MCP: {captured}"
    );
}

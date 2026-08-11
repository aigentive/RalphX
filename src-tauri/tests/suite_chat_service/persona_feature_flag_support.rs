pub(super) use std::fs;
pub(super) use std::sync::{Arc, Mutex};

pub(super) use ralphx_lib::application::app_paths::AppPaths;
pub(super) use ralphx_lib::application::chat_service::{
    process_queued_messages_for_test, process_queued_messages_for_test_with_persona_feature,
    AppChatService, ChatService, ChatServiceError, SendMessageOptions,
    PERSONA_BUILDER_FEATURE_DISABLED_ERROR,
};
pub(super) use ralphx_lib::application::persona_ingest::{
    persona_ingest_conversation_path, persona_ingest_storage_path,
};
pub(super) use ralphx_lib::application::standalone_workspace::create_workspace;
pub(super) use ralphx_lib::application::AppState;
pub(super) use ralphx_lib::domain::agents::AgentHarnessKind;
pub(super) use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, AgentRun, AgentRunId, ChatContextType, ChatConversation,
    ChatConversationId, Persona, PersonaId, PersonaStatus, Project,
};
pub(super) use ralphx_lib::domain::repositories::ChatConversationRepository;
pub(super) use ralphx_lib::infrastructure::agents::{
    reset_agent_personas_override_for_test, reset_standalone_conversations_override_for_test,
    set_agent_personas_override, set_standalone_conversations_override,
};
pub(super) use ralphx_lib::infrastructure::memory::MemoryChatConversationRepository;
pub(super) use ralphx_lib::utils::path_safety::validate_absolute_non_root_path;
pub(super) use tauri::{Listener, Manager};

pub(super) use super::support::{fake_claude::FakeClaude, fake_codex::FakeCodex};

pub(super) struct PersonaEnvReset {
    key: &'static str,
    previous: Option<String>,
}

impl PersonaEnvReset {
    pub(super) fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for PersonaEnvReset {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_ref() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

pub(super) struct PersonaFlagOverrideReset;

impl Drop for PersonaFlagOverrideReset {
    fn drop(&mut self) {
        reset_agent_personas_override_for_test();
    }
}

pub(super) struct StandaloneFlagOverrideReset;

impl Drop for StandaloneFlagOverrideReset {
    fn drop(&mut self) {
        reset_standalone_conversations_override_for_test();
    }
}

pub(super) fn persona_flag_override_chat_service(state: &AppState) -> AppChatService {
    AppChatService::new(
        Arc::clone(&state.chat_message_repo),
        Arc::clone(&state.chat_attachment_repo),
        Arc::clone(&state.artifact_repo),
        Arc::clone(&state.chat_conversation_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.task_repo),
        Arc::clone(&state.task_dependency_repo),
        Arc::clone(&state.ideation_session_repo),
        Arc::clone(&state.delegated_session_repo),
        Arc::clone(&state.activity_event_repo),
        Arc::clone(&state.message_queue),
        Arc::clone(&state.running_agent_registry),
        Arc::clone(&state.memory_event_repo),
    )
}

pub(super) async fn seed_completed_continuation_runtime(
    state: &AppState,
    conversation_id: &ChatConversationId,
    harness: AgentHarnessKind,
    provider_session_id: &str,
) {
    let mut run = AgentRun::new(*conversation_id);
    run.complete();
    run.harness = Some(harness);
    run.provider_session_id = Some(provider_session_id.to_string());
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed completed continuation runtime");
}

#[cfg(unix)]
pub(super) async fn send_persona_attribution_fixture(
    persona: Option<Persona>,
    native_agent_flag: bool,
) -> (
    ralphx_lib::domain::entities::AgentRun,
    Vec<serde_json::Value>,
) {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;
    use tauri::Listener;

    let _spawn_permission = PersonaEnvReset::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let _native_agent_flag = PersonaEnvReset::set(
        "RALPHX_USE_NATIVE_AGENT_FLAG",
        if native_agent_flag { "1" } else { "0" },
    );
    let temp = tempfile::tempdir().expect("persona attribution temp directory");
    let project_directory = temp.path().join("project");
    fs::create_dir_all(&project_directory).expect("create persona attribution project");
    let cli_path = temp.path().join("fake-claude");
    fs::write(
        &cli_path,
        // Drain stdin before exiting: the send path writes the prompt to the
        // child's stdin, and a CLI that exits first gets EPIPE on Linux (CI).
        "#!/bin/sh\ncat > /dev/null 2>&1\nprintf '%s\\n' '{\"type\":\"result\",\"session_id\":\"persona-session\",\"is_error\":false,\"result\":\"ok\",\"cost_usd\":0.0}'\n",
    )
    .expect("write persona attribution fake CLI");
    let mut permissions = fs::metadata(&cli_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&cli_path, permissions).unwrap();

    let initial_state = AppState::new_test();
    if let Some(persona) = persona.as_ref() {
        initial_state
            .persona_repo
            .create(persona.clone())
            .await
            .expect("persist persona attribution fixture");
    }
    let project = initial_state
        .project_repo
        .create(Project::new(
            "Persona Attribution".to_string(),
            project_directory.to_string_lossy().into_owned(),
        ))
        .await
        .expect("persist persona attribution project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.persona_id = persona.as_ref().map(|value| value.id.to_string());
    let conversation = initial_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist persona attribution conversation");

    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build persona attribution mock app");
    let events = Arc::new(Mutex::new(Vec::new()));
    for event_name in ["persona:applied", "persona:injection_skipped"] {
        let captured = Arc::clone(&events);
        let _ = app.listen(event_name, move |event| {
            if let Ok(payload) = serde_json::from_str(event.payload()) {
                captured.lock().unwrap().push(payload);
            }
        });
    }

    let service = app
        .state::<AppState>()
        .build_chat_service_for_runtime::<tauri::test::MockRuntime>(
            None,
            Some(app.handle().clone()),
        )
        .with_persona_feature_enabled(true)
        .with_cli_path(cli_path)
        .with_working_directory(&project_directory);
    let result = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Run the body-free persona attribution fixture.",
            SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                ..Default::default()
            },
        )
        .await
        .expect("persona attribution send should spawn");
    tokio::task::yield_now().await;
    let run = app
        .state::<AppState>()
        .agent_run_repo
        .get_by_id(&AgentRunId::from_string(result.agent_run_id))
        .await
        .expect("read persona attribution run")
        .expect("persona attribution run should exist");
    let captured = events.lock().unwrap().clone();
    (run, captured)
}

pub(super) fn persona_attribution_fixture() -> Persona {
    Persona {
        id: PersonaId::from("persona-design-voice"),
        artifact_id: None,

        project_id: None,
        slug: "design-voice".to_string(),
        name: "Design Voice".to_string(),
        description: "Persona attribution fixture".to_string(),
        content: "SECRET_PERSONA_BODY_SENTINEL".to_string(),
        status: PersonaStatus::Active,
        version: 2,
        content_hash: "persona-content-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

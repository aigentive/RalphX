use std::fs;
use std::sync::Arc;

use ralphx_lib::application::app_paths::AppPaths;
use ralphx_lib::application::chat_service::{
    AppChatService, ChatService, ChatServiceError, SendMessageOptions,
};
use ralphx_lib::application::persona_ingest::{
    persona_ingest_conversation_path, persona_ingest_storage_path,
};
use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, AgentRunId, ChatContextType, ChatConversation, Persona,
    PersonaId, PersonaStatus, Project,
};
use ralphx_lib::infrastructure::agents::claude::{
    reset_agent_personas_override_for_test, set_agent_personas_override,
};
use tauri::Manager;

struct PersonaEnvReset {
    key: &'static str,
    previous: Option<String>,
}

impl PersonaEnvReset {
    fn set(key: &'static str, value: &str) -> Self {
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

struct PersonaFlagOverrideReset;

impl Drop for PersonaFlagOverrideReset {
    fn drop(&mut self) {
        reset_agent_personas_override_for_test();
    }
}

fn persona_flag_override_chat_service(state: &AppState) -> AppChatService {
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

#[test]
fn persona_flag_override_chat_service_keeps_builder_override_and_live_default() {
    let _reset = PersonaFlagOverrideReset;
    set_agent_personas_override(Some(true));
    let state = AppState::new_test();

    assert!(persona_flag_override_chat_service(&state).persona_feature_enabled_for_test());
    assert!(
        !persona_flag_override_chat_service(&state)
            .with_persona_feature_enabled(false)
            .persona_feature_enabled_for_test(),
        "the explicit test seam must override the live feature flag"
    );
}

#[tokio::test]
async fn persona_builder_send_requires_live_ingest_then_reaches_the_next_guard() {
    let temp = tempfile::tempdir().expect("persona builder send temp directory");
    let app_data_dir = temp.path().join("app-data");
    let project_directory = temp.path().join("project");
    fs::create_dir_all(&project_directory).expect("create persona builder project directory");

    let mut initial_state = AppState::new_test();
    initial_state.app_paths = AppPaths::new(app_data_dir.clone(), None);
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build persona builder mock app");
    let state = app.state::<AppState>();
    let project = state
        .project_repo
        .create(Project::new(
            "Persona Builder Send".to_string(),
            project_directory.to_string_lossy().into_owned(),
        ))
        .await
        .expect("create persona builder project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create persona builder conversation");
    let service = state
        .build_chat_service_for_runtime::<tauri::test::MockRuntime>(
            None,
            Some(app.handle().clone()),
        )
        .with_persona_feature_enabled(true);
    let options = SendMessageOptions {
        conversation_id_override: Some(conversation.id),
        ..Default::default()
    };

    let rejected = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Draft a focused reviewer persona.",
            options.clone(),
        )
        .await
        .expect_err("a PersonaBuilder send without ingest context must fail closed");
    assert!(matches!(
        rejected,
        ChatServiceError::PersonaUnavailable(message)
            if message == "[Persona unavailable: PersonaBuilder requires a live draft ingest session]"
    ));

    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        &conversation.id.as_str(),
    );
    fs::create_dir_all(&ingest_root).expect("create live persona builder ingest root");
    fs::write(ingest_root.join("content"), "approved persona context")
        .expect("write live persona builder ingest content");

    let after_ingest = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Draft a focused reviewer persona.",
            options,
        )
        .await;
    assert!(
        !matches!(after_ingest, Err(ChatServiceError::PersonaUnavailable(_))),
        "a live ingest session must pass the PersonaBuilder guard"
    );
}

#[cfg(unix)]
async fn send_persona_attribution_fixture(
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

fn persona_attribution_fixture() -> Persona {
    Persona {
        id: PersonaId::from("persona-design-voice"),
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

#[cfg(unix)]
#[tokio::test]
async fn persona_normal_send_persists_applied_attribution_without_body_and_emits_body_free_event() {
    let (run, events) =
        send_persona_attribution_fixture(Some(persona_attribution_fixture()), false).await;

    assert_eq!(run.persona_id.as_deref(), Some("persona-design-voice"));
    assert_eq!(run.persona_slug.as_deref(), Some("design-voice"));
    assert_eq!(run.persona_version, Some(2));
    assert_eq!(
        run.persona_content_hash.as_deref(),
        Some("persona-content-hash")
    );
    assert_eq!(run.persona_injected, Some(true));
    assert_eq!(run.persona_skipped_reason, None);
    let serialized_run = serde_json::to_string(&run).unwrap();
    assert!(!serialized_run.contains("SECRET_PERSONA_BODY_SENTINEL"));
    let applied = events
        .iter()
        .find(|payload| payload["persona_slug"] == "design-voice")
        .expect("persona applied event should be emitted");
    assert_eq!(applied["persona_id"], "persona-design-voice");
    assert_eq!(applied["version"], 2);
    assert_eq!(applied["run_id"], run.id.as_str());
    assert!(!applied.to_string().contains("SECRET_PERSONA_BODY_SENTINEL"));
}

#[cfg(unix)]
#[tokio::test]
async fn persona_native_agent_skip_persists_not_injected_reason() {
    let (run, events) =
        send_persona_attribution_fixture(Some(persona_attribution_fixture()), true).await;

    assert_eq!(run.persona_injected, Some(false));
    assert_eq!(
        run.persona_skipped_reason.as_deref(),
        Some("native_agent_flag")
    );
    let skipped = events
        .iter()
        .find(|payload| payload["reason"] == "native_agent_flag")
        .expect("persona skipped event should be emitted");
    assert_eq!(skipped["run_id"], run.id.as_str());
    assert_eq!(skipped["persona_slug"], "design-voice");
    assert!(!skipped.to_string().contains("SECRET_PERSONA_BODY_SENTINEL"));
}

#[cfg(unix)]
#[tokio::test]
async fn persona_absent_send_leaves_all_run_attribution_columns_null() {
    let (run, events) = send_persona_attribution_fixture(None, false).await;

    assert_eq!(run.persona_id, None);
    assert_eq!(run.persona_slug, None);
    assert_eq!(run.persona_version, None);
    assert_eq!(run.persona_content_hash, None);
    assert_eq!(run.persona_injected, None);
    assert_eq!(run.persona_skipped_reason, None);
    assert!(events.is_empty());
}

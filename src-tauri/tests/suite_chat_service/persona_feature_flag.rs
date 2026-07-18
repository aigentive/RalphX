use std::fs;
use std::sync::{Arc, Mutex};

use ralphx_lib::application::app_paths::AppPaths;
use ralphx_lib::application::chat_service::{
    process_queued_messages_for_test, validate_conversation_spawn_harness, AppChatService,
    ChatService, ChatServiceError, SendMessageOptions, STANDALONE_PERSONA_BUILDER_CODEX_ERROR,
};
use ralphx_lib::application::persona_ingest::{
    persona_ingest_conversation_path, persona_ingest_storage_path,
};
use ralphx_lib::application::standalone_workspace::create_workspace;
use ralphx_lib::application::AppState;
use ralphx_lib::domain::agents::AgentHarnessKind;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, AgentRunId, ChatContextType, ChatConversation, Persona,
    PersonaId, PersonaStatus, Project,
};
use ralphx_lib::domain::repositories::ChatConversationRepository;
use ralphx_lib::infrastructure::agents::claude::{
    reset_agent_personas_override_for_test, reset_standalone_conversations_override_for_test,
    set_agent_personas_override, set_standalone_conversations_override,
};
use ralphx_lib::infrastructure::memory::MemoryChatConversationRepository;
use ralphx_lib::utils::path_safety::validate_absolute_non_root_path;
use tauri::{Listener, Manager};

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

struct StandaloneFlagOverrideReset;

impl Drop for StandaloneFlagOverrideReset {
    fn drop(&mut self) {
        reset_standalone_conversations_override_for_test();
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
async fn persona_builder_send_no_longer_requires_live_ingest() {
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

    let without_ingest = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Draft a focused reviewer persona.",
            options.clone(),
        )
        .await;
    assert!(
        !matches!(without_ingest, Err(ChatServiceError::PersonaUnavailable(_))),
        "Phase 5 retires the ingest-liveness send gate unconditionally"
    );

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
        "legacy ingest presence must not change the retired gate's behavior"
    );
}

#[tokio::test]
async fn persona_builder_send_does_not_reintroduce_the_retired_ingest_gate() {
    let temp = tempfile::tempdir().expect("bound persona builder send temp directory");
    let app_data_dir = temp.path().join("app-data");
    let project_directory = temp.path().join("project");
    fs::create_dir_all(&project_directory).expect("create bound builder project directory");

    let mut initial_state = AppState::new_test();
    initial_state.app_paths = AppPaths::new(app_data_dir.clone(), None);
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build bound persona builder mock app");
    let state = app.state::<AppState>();
    let project = state
        .project_repo
        .create(Project::new(
            "Bound Persona Builder Send".to_string(),
            project_directory.to_string_lossy().into_owned(),
        ))
        .await
        .expect("create bound persona builder project");
    let draft_id = PersonaId::new();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    conversation.builder_draft_id = Some(draft_id.as_str().to_string());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create bound persona builder conversation");
    let now = chrono::Utc::now();
    state
        .persona_repo
        .create(Persona {
            id: draft_id.clone(),
            artifact_id: None,

            project_id: None,
            slug: "send-bound-draft".to_string(),
            name: "send-bound-draft".to_string(),
            description: "Bound send fixture".to_string(),
            content: "---\nname: send-bound-draft\nkind: persona\ndescription: Bound send fixture\n---\nBody".to_string(),
            status: PersonaStatus::Draft,
            version: 1,
            content_hash: "bound-send-hash".to_string(),
            source_session_id: Some(conversation.id.as_str().to_string()),
            source_persona_id: None,
            source_content_hash: None,
            source_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("create live bound draft");
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

    let with_draft = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Revise the bound persona without filesystem context.",
            options.clone(),
        )
        .await;
    assert!(
        !matches!(with_draft, Err(ChatServiceError::PersonaUnavailable(_))),
        "a valid bound Draft must pass every normal send guard"
    );

    state
        .persona_repo
        .set_status(&draft_id, PersonaStatus::Active)
        .await
        .expect("make bound row non-Draft");
    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        &conversation.id.as_str(),
    );
    fs::create_dir_all(&ingest_root).expect("create ingest root beside non-Draft binding");
    fs::write(ingest_root.join("context"), "Must not mask invalid binding")
        .expect("write ingest context beside non-Draft binding");
    let non_draft = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "This send must continue past the retired ingest gate.",
            options.clone(),
        )
        .await;
    assert!(
        !matches!(non_draft, Err(ChatServiceError::PersonaUnavailable(_))),
        "draft status is enforced by draft-write paths, not the retired send gate"
    );

    state.persona_repo.delete(&draft_id).await.unwrap();
    let dangling = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "This dangling binding must also continue past the retired gate.",
            options,
        )
        .await;
    assert!(
        matches!(
            dangling,
            Err(ChatServiceError::PersonaUnavailable(ref message))
                if message != "[Persona unavailable: PersonaBuilder requires ingested context or a live bound draft]"
        ),
        "a dangling binding remains fail-closed without resurrecting the ingest-era reason: {dangling:?}"
    );
}

#[tokio::test]
async fn standalone_persona_builder_fresh_send_rejects_codex_override() {
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
            conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
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
            "Do not switch this builder to Codex.",
            SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                harness_override: Some(AgentHarnessKind::Codex),
                ..Default::default()
            },
        )
        .await
        .expect_err("standalone builder Codex send must reject");

    assert!(matches!(
        error,
        ChatServiceError::SpawnFailed(ref message)
            if message == STANDALONE_PERSONA_BUILDER_CODEX_ERROR
    ));
}

#[test]
fn codex_send_guard_allows_project_builder_and_standalone_chat() {
    let mut project_builder = ChatConversation::new_project(
        ralphx_lib::domain::entities::ProjectId::from_string("project-builder-codex".to_string()),
    );
    project_builder.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    validate_conversation_spawn_harness(&project_builder, AgentHarnessKind::Codex)
        .expect("Project-context builder must still allow Codex sends");

    let mut standalone_chat = ChatConversation::new_standalone();
    standalone_chat.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    validate_conversation_spawn_harness(&standalone_chat, AgentHarnessKind::Codex)
        .expect("Standalone chat must still allow Codex sends");
}

#[tokio::test]
async fn standalone_builder_queue_rejects_codex_override_with_agent_error() {
    let _standalone_reset = StandaloneFlagOverrideReset;
    set_standalone_conversations_override(Some(true));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let mut conversation = ChatConversation::new_standalone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
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
        .any(|payload| payload.contains(STANDALONE_PERSONA_BUILDER_CODEX_ERROR)));
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
    let context_id = conversation.id.as_str();
    initial_state
        .message_queue
        .queue_with_runtime_overrides_and_project_references(
            ChatContextType::Project,
            &context_id,
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

    let (processed, last_run_id) = process_queued_messages_for_test(
        app.handle().clone(),
        ChatContextType::Project,
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
    let mut conversation = ChatConversation::new_project(project.id);
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = initial_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("persist queued builder conversation");
    let context_id = conversation.id.as_str();
    initial_state.message_queue.queue(
        ChatContextType::Project,
        context_id.clone(),
        "drain queued builder with enforcement".to_string(),
    );
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build queued builder enforcement app");

    let (processed, _) = process_queued_messages_for_test(
        app.handle().clone(),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        &context_id,
        conversation.id,
        "queued-builder-old-session",
        &cli_path,
    )
    .await;

    assert_eq!(processed, 1);
    let captured = fs::read_to_string(capture_path)
        .expect("read queued builder spawn arguments and MCP config");
    assert!(
        captured.contains("--filesystem-enforced") && captured.contains("\"1\""),
        "queued PersonaBuilder drain must pass filesystem enforcement to MCP: {captured}"
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

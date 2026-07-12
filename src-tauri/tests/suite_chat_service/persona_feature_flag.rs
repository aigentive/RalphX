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
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, Project,
};
use ralphx_lib::infrastructure::agents::claude::{
    reset_agent_personas_override_for_test, set_agent_personas_override,
};
use tauri::Manager;

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

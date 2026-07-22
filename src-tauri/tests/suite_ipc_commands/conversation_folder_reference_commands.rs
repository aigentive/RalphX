use std::sync::Arc;

use ralphx_lib::application::{AppPaths, AppState};
use ralphx_lib::commands::conversation_folder_reference_commands::{
    add_conversation_folder_reference_for_state, list_conversation_folder_references_for_state,
    remove_conversation_folder_reference_for_state, AddConversationFolderReferenceInput,
    RemoveConversationFolderReferenceInput,
};
use ralphx_lib::commands::unified_chat_commands::{
    create_agent_conversation, CreateAgentConversationInput,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, ChatConversationId, IdeationSessionId,
    ProjectId,
};
use ralphx_lib::error::AppError;
use ralphx_lib::infrastructure::agents::{
    reset_agent_personas_override_for_test, reset_standalone_conversations_override_for_test,
    set_agent_personas_override, set_standalone_conversations_override,
};
use ralphx_lib::infrastructure::memory::MemoryConversationFolderReferenceRepository;
use ralphx_lib::utils::path_safety::validate_absolute_non_root_path;
use tauri::Manager;

struct PersonaFlagOverrideReset;

impl Drop for PersonaFlagOverrideReset {
    fn drop(&mut self) {
        reset_agent_personas_override_for_test();
        reset_standalone_conversations_override_for_test();
    }
}

#[tokio::test]
async fn folder_reference_commands_run_through_managed_tauri_state() {
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("temp directory");
    let app_data = validate_absolute_non_root_path(
        &temp.path().join("app-data"),
        "folder reference command test app data",
    )
    .expect("safe app data path");
    let folder = validate_absolute_non_root_path(
        &temp.path().join("folder"),
        "folder reference command test folder",
    )
    .expect("safe folder path");
    std::fs::create_dir(&app_data).expect("create app data");
    std::fs::create_dir(&folder).expect("create folder");

    let mut state = AppState::new_test();
    state.app_paths = AppPaths::new(app_data, None);
    state.conversation_folder_reference_repo =
        Arc::new(MemoryConversationFolderReferenceRepository::new());
    let conversation = ChatConversation::new_project(ProjectId::new());
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed project conversation");

    let created = add_conversation_folder_reference_for_state(
        AddConversationFolderReferenceInput {
            conversation_id: conversation_id.as_str(),
            folder_path: folder.to_string_lossy().into_owned(),
            display_name: "Command Folder".to_string(),
        },
        &state,
    )
    .await
    .expect("add command succeeds");
    let listed = list_conversation_folder_references_for_state(conversation_id.as_str(), &state)
        .await
        .expect("list command succeeds");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);

    remove_conversation_folder_reference_for_state(
        RemoveConversationFolderReferenceInput {
            conversation_id: conversation_id.as_str(),
            folder_reference_id: created.id,
        },
        &state,
    )
    .await
    .expect("remove command succeeds");
    assert!(
        list_conversation_folder_references_for_state(conversation_id.as_str(), &state)
            .await
            .expect("list after remove succeeds")
            .is_empty()
    );
}

#[tokio::test]
async fn folder_reference_commands_always_allow_supported_contexts_and_reject_others() {
    let _persona_reset = PersonaFlagOverrideReset;
    set_agent_personas_override(Some(true));
    set_standalone_conversations_override(Some(true));
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("temp directory");
    let app_data = validate_absolute_non_root_path(
        &temp.path().join("app-data"),
        "folder reference gate app data",
    )
    .expect("safe app data");
    let folder = validate_absolute_non_root_path(
        &temp.path().join("folder"),
        "folder reference gate folder",
    )
    .expect("safe folder");
    std::fs::create_dir(&app_data).expect("create app data");
    std::fs::create_dir(&folder).expect("create folder");
    let mut initial_state = AppState::new_test();
    initial_state.app_paths = AppPaths::new(app_data, None);
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("folder-reference gate app");
    let state = app.state::<AppState>();

    let project = ChatConversation::new_project(ProjectId::new());
    let project_id = project.id;
    state.chat_conversation_repo.create(project).await.unwrap();
    let mut ideation = ChatConversation::new_ideation(IdeationSessionId::new());
    ideation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let ideation_id = ideation.id;
    state.chat_conversation_repo.create(ideation).await.unwrap();
    let builder_id = ChatConversationId::from_string(
        create_agent_conversation(
            CreateAgentConversationInput {
                context_type: "project".to_string(),
                context_id: Some(ProjectId::new().as_str().to_string()),
                title: None,
                mode: Some("persona_builder".to_string()),
                team_intent: None,
            },
            app.state(),
        )
        .await
        .expect("production Project builder seed")
        .id,
    );
    let standalone_builder_id = ChatConversationId::from_string(
        create_agent_conversation(
            CreateAgentConversationInput {
                context_type: "standalone".to_string(),
                context_id: None,
                title: None,
                mode: Some("persona_builder".to_string()),
                team_intent: None,
            },
            app.state(),
        )
        .await
        .expect("production Standalone builder seed")
        .id,
    );

    let input = |conversation_id: ChatConversationId| AddConversationFolderReferenceInput {
        conversation_id: conversation_id.as_str(),
        folder_path: folder.to_string_lossy().into_owned(),
        display_name: "Folder".to_string(),
    };
    assert!(matches!(
        add_conversation_folder_reference_for_state(input(ideation_id), &state).await,
        Err(AppError::ConversationFolderReferenceUnsupportedContext)
    ));
    add_conversation_folder_reference_for_state(input(builder_id), &state)
        .await
        .expect("Project builder add succeeds");
    add_conversation_folder_reference_for_state(input(standalone_builder_id), &state)
        .await
        .expect("Standalone builder add succeeds");
    let created = add_conversation_folder_reference_for_state(input(project_id), &state)
        .await
        .expect("enabled Project non-builder add succeeds");
    assert_eq!(
        list_conversation_folder_references_for_state(project_id.as_str(), &state)
            .await
            .expect("always-on list succeeds")
            .len(),
        1,
    );
    remove_conversation_folder_reference_for_state(
        RemoveConversationFolderReferenceInput {
            conversation_id: project_id.as_str(),
            folder_reference_id: created.id,
        },
        &state,
    )
    .await
    .expect("always-on remove succeeds");
}

#[test]
fn folder_reference_command_inputs_require_camel_case_fields() {
    let input: AddConversationFolderReferenceInput = serde_json::from_str(
        r#"{"conversationId":"conversation","folderPath":"/folder","displayName":"Folder"}"#,
    )
    .expect("camelCase input deserializes");
    assert_eq!(input.conversation_id, "conversation");
    assert_eq!(input.folder_path, "/folder");
    assert_eq!(input.display_name, "Folder");
}

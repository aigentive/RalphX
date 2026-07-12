use ralphx_lib::application::personas::PERSONA_FEATURE_DISABLED_PREFIX;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::persona_builder_commands::{
    create_persona_builder_conversation_for_state, CreatePersonaBuilderConversationInput,
};
use ralphx_lib::domain::entities::{AgentConversationWorkspaceMode, ProjectId};

#[tokio::test]
async fn builder_conversation_created_only_via_flag_gated_settings_command() {
    let state = AppState::new_test();
    let input = CreatePersonaBuilderConversationInput {
        project_id: "project-persona-builder-command".to_string(),
    };

    let disabled = create_persona_builder_conversation_for_state(input.clone(), &state, false)
        .await
        .expect_err("flag-off PersonaBuilder creation must return the typed disabled error");
    assert!(
        disabled.contains(PERSONA_FEATURE_DISABLED_PREFIX),
        "unexpected disabled error: {disabled}"
    );

    let response = create_persona_builder_conversation_for_state(input, &state, true)
        .await
        .expect("flag-on PersonaBuilder creation should persist its conversation");
    assert_eq!(response.context_id, "project-persona-builder-command");
    assert_eq!(response.agent_mode.as_deref(), Some("persona_builder"));
    assert_eq!(response.title.as_deref(), Some("Persona builder"));

    let stored = state
        .chat_conversation_repo
        .get_by_id(&ralphx_lib::domain::entities::ChatConversationId::from_string(response.id))
        .await
        .expect("conversation lookup succeeds")
        .expect("created conversation persists");
    assert_eq!(
        stored.agent_mode,
        Some(AgentConversationWorkspaceMode::PersonaBuilder)
    );
    assert_eq!(stored.title.as_deref(), Some("Persona builder"));
    assert!(state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&stored.id)
        .await
        .expect("workspace lookup succeeds")
        .is_none());

    let project_id = ProjectId::from_string(stored.context_id);
    assert_eq!(project_id.as_str(), "project-persona-builder-command");
}

#[test]
fn persona_builder_command_input_uses_camel_case_project_id() {
    let input: CreatePersonaBuilderConversationInput =
        serde_json::from_str(r#"{"projectId":"project-persona-builder-input"}"#)
            .expect("camelCase projectId should deserialize");
    assert_eq!(input.project_id, "project-persona-builder-input");
}

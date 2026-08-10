use chrono::Utc;
use std::sync::Arc;

use crate::application::AppState;
use crate::commands::agent_conversation_mute_commands::{
    set_agent_conversation_muted_for_app_state, SetAgentConversationMutedInput,
};
use crate::commands::unified_chat_commands::{
    switch_agent_conversation_persona_for_state_rejecting_running_agent_with_flags,
    SwitchAgentConversationPersonaInput, PERSONA_SWITCH_AGENT_RUNNING_ERROR,
};
use crate::commands::ExecutionState;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatContextType, ChatConversation,
    IdeationAnalysisBaseRefKind, Persona, PersonaId, PersonaStatus, ProjectId,
};
use crate::domain::services::RunningAgentKey;

#[tokio::test]
async fn remote_mute_persists_the_same_row_shape_as_local_mute() {
    let local = AppState::new_test();
    let remote = AppState::new_test();
    let conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    let conversation_id = conversation.id.clone();
    local
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("local conversation");
    remote
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("remote conversation");
    let workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Chat,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        Some("Current branch (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project-1/conversation".to_string(),
        "/tmp/remote-conversation-lifecycle".to_string(),
    );
    local
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("local workspace");
    remote
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("remote workspace");

    set_agent_conversation_muted_for_app_state(
        SetAgentConversationMutedInput {
            conversation_id: conversation_id.to_string(),
            muted: true,
        },
        &local,
        &Arc::new(ExecutionState::new()),
    )
    .await
    .expect("local mute");
    crate::commands::agent_conversation_mute_commands::set_agent_conversation_muted_for_state_without_repair_recovery(
        SetAgentConversationMutedInput {
            conversation_id: conversation_id.to_string(),
            muted: true,
        },
        &remote,
    )
    .await
    .expect("remote mute");

    let local_row = local
        .agent_conversation_mute_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("local row")
        .expect("local mute exists");
    let remote_row = remote
        .agent_conversation_mute_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("remote row")
        .expect("remote mute exists");
    assert_eq!(local_row.conversation_id, remote_row.conversation_id);
    assert_eq!(local_row.state_fingerprint, remote_row.state_fingerprint);
}

fn active_persona(id: &str) -> Persona {
    Persona {
        id: PersonaId::from(id),
        artifact_id: None,
        project_id: None,
        slug: id.to_string(),
        name: id.to_string(),
        description: "remote persona fixture".to_string(),
        content: "Be precise".to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: format!("{id}-hash"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

async fn persona_fixture() -> (
    AppState,
    crate::domain::entities::ChatConversationId,
    PersonaId,
) {
    let state = AppState::new_test();
    let conversation =
        ChatConversation::new_project(ProjectId::from_string("project-persona".to_string()));
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation");
    let persona = active_persona("remote-persona");
    let persona_id = persona.id.clone();
    state.persona_repo.create(persona).await.expect("persona");
    (state, conversation_id, persona_id)
}

#[tokio::test]
async fn remote_persona_switch_rejects_running_agent_without_writing_binding() {
    let (state, conversation_id, persona_id) = persona_fixture().await;
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                conversation_id.as_str(),
            ),
            0,
            conversation_id.to_string(),
            "remote-persona-run".to_string(),
            None,
            None,
        )
        .await;

    let error = switch_agent_conversation_persona_for_state_rejecting_running_agent_with_flags(
        SwitchAgentConversationPersonaInput {
            conversation_id: conversation_id.to_string(),
            persona_id: Some(persona_id.to_string()),
        },
        &state,
        true,
        false,
    )
    .await
    .expect_err("running switch must be rejected");
    assert_eq!(error, PERSONA_SWITCH_AGENT_RUNNING_ERROR);
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("lookup")
        .expect("conversation");
    assert!(stored.persona_id.is_none());
}

#[tokio::test]
async fn remote_persona_switch_writes_binding_and_resets_provider_session_when_idle() {
    let (state, conversation_id, persona_id) = persona_fixture().await;
    state
        .chat_conversation_repo
        .update_provider_session_ref(
            &conversation_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "old-provider-session".to_string(),
            },
        )
        .await
        .expect("provider session seed");

    switch_agent_conversation_persona_for_state_rejecting_running_agent_with_flags(
        SwitchAgentConversationPersonaInput {
            conversation_id: conversation_id.to_string(),
            persona_id: Some(persona_id.to_string()),
        },
        &state,
        true,
        true,
    )
    .await
    .expect("idle switch succeeds");
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("lookup")
        .expect("conversation");
    assert_eq!(stored.persona_id.as_deref(), Some(persona_id.as_str()));
    assert!(stored.provider_session_ref().is_none());
}

#[tokio::test]
async fn remote_persona_switch_refuses_when_personas_are_disabled() {
    let (state, conversation_id, persona_id) = persona_fixture().await;

    let error = switch_agent_conversation_persona_for_state_rejecting_running_agent_with_flags(
        SwitchAgentConversationPersonaInput {
            conversation_id: conversation_id.to_string(),
            persona_id: Some(persona_id.to_string()),
        },
        &state,
        false,
        false,
    )
    .await
    .expect_err("disabled flag must reject");
    assert!(error.contains("agent personas feature is disabled"));
}

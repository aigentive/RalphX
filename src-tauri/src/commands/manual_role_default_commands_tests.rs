use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, ManualRoleDefault, ManualServiceTier, RoutingRole};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, CoordinationMode, Project,
};

use super::manual_role_default_commands::{
    control_options, parse_input, reset_agent_conversation_role_default_for_state,
    ManualRoleDefaultInput, ResetAgentConversationRoleDefaultInput,
};

#[test]
fn non_workspace_roles_receive_backend_disabled_capability_and_persona_reasons() {
    let options = control_options(RoutingRole::ExecutionWorker, AgentHarnessKind::Codex, true);

    assert!(
        options
            .capabilities
            .iter()
            .find(|option| option.value == "solo")
            .unwrap()
            .enabled
    );
    assert!(
        !options
            .capabilities
            .iter()
            .find(|option| option.value == "rx_native_team")
            .unwrap()
            .enabled
    );
    assert!(!options.persona.enabled);
    assert!(options.persona.disabled_reason.is_some());
}

#[test]
fn parses_explicit_standard_solo_and_persona_without_collapsing_them() {
    let value = parse_input(ManualRoleDefaultInput {
        provider: "codex".into(),
        model: Some("gpt-5.6".into()),
        effort: Some("xhigh".into()),
        service_tier: "standard".into(),
        coordination_mode: Some("solo".into()),
        persona_id: Some("persona-1".into()),
        approval_policy: Some("never".into()),
        sandbox_mode: Some("danger-full-access".into()),
    })
    .unwrap();

    assert_eq!(value.service_tier.to_string(), "standard");
    assert_eq!(value.coordination_mode.unwrap().to_string(), "solo");
    assert_eq!(value.persona_id.unwrap().as_str(), "persona-1");
}

#[tokio::test]
async fn active_conversation_reset_applies_complete_role_binding_together() {
    let state = AppState::new_test();
    let project_root = tempfile::tempdir().unwrap();
    let project = state
        .project_repo
        .create(Project::new(
            "Reset project".into(),
            project_root.path().to_string_lossy().into_owned(),
        ))
        .await
        .unwrap();
    state
        .manual_role_default_repo
        .upsert_for_project(
            project.id.as_str(),
            RoutingRole::WorkspaceEdit,
            &ManualRoleDefault {
                harness: AgentHarnessKind::Claude,
                model: Some("sonnet".into()),
                effort: None,
                service_tier: ManualServiceTier::Standard,
                coordination_mode: Some(CoordinationMode::Solo),
                persona_id: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    conversation.set_coordination_mode(CoordinationMode::RxNativeTeam);
    conversation.persona_id = Some("stale-persona".into());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let response = reset_agent_conversation_role_default_for_state(
        ResetAgentConversationRoleDefaultInput {
            conversation_id: conversation.id.as_str().to_string(),
        },
        &state,
        None,
    )
    .await
    .unwrap();

    assert_eq!(response.role, "workspace_edit");
    assert_eq!(response.value.service_tier, "standard");
    let reset = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reset.coordination_mode, CoordinationMode::Solo);
    assert_eq!(reset.persona_id, None);
}

#[tokio::test]
async fn rejected_active_conversation_reset_leaves_all_role_bindings_unchanged() {
    let state = AppState::new_test();
    let project_root = tempfile::tempdir().unwrap();
    let project = state
        .project_repo
        .create(Project::new(
            "Rejected reset project".into(),
            project_root.path().to_string_lossy().into_owned(),
        ))
        .await
        .unwrap();
    state
        .manual_role_default_repo
        .upsert_for_project(
            project.id.as_str(),
            RoutingRole::WorkspaceEdit,
            &ManualRoleDefault {
                harness: AgentHarnessKind::Claude,
                model: Some("sonnet".into()),
                effort: None,
                service_tier: ManualServiceTier::Standard,
                coordination_mode: Some(CoordinationMode::RxNativeTeam),
                persona_id: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    conversation.set_coordination_mode(CoordinationMode::Solo);
    conversation.persona_id = Some("keep-persona".into());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let error = reset_agent_conversation_role_default_for_state(
        ResetAgentConversationRoleDefaultInput {
            conversation_id: conversation.id.as_str().to_string(),
        },
        &state,
        None,
    )
    .await
    .unwrap_err();

    assert!(error.contains("Team is disabled"));
    let unchanged = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.coordination_mode, CoordinationMode::Solo);
    assert_eq!(unchanged.persona_id.as_deref(), Some("keep-persona"));
}

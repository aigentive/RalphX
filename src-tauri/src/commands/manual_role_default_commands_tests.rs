use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, ManualRoleDefault, ManualServiceTier, RoutingRole};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, CoordinationMode, Project,
};

use super::manual_role_default_commands::{
    control_options, parse_input, reset_agent_conversation_role_default_for_state,
    update_manual_role_default_for_state, ManualRoleDefaultInput,
    ResetAgentConversationRoleDefaultInput, UpdateManualRoleDefaultInput,
};

#[test]
fn non_workspace_roles_receive_backend_disabled_capability_and_persona_reasons() {
    let options = control_options(
        RoutingRole::ExecutionWorker,
        AgentHarnessKind::Codex,
        Some("unsupported-model"),
        true,
        &crate::application::agent_capability_gate::AgentCapabilityGate::default(),
    );

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
fn unsupported_codex_model_disables_ultra_in_role_control_metadata() {
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
    let options = control_options(
        RoutingRole::WorkspaceEdit,
        AgentHarnessKind::Codex,
        Some("definitely-not-an-ultra-model"),
        true,
        &crate::application::agent_capability_gate::AgentCapabilityGate::default(),
    );
    let ultra = options
        .capabilities
        .iter()
        .find(|option| option.value == "codex_native_ultra")
        .unwrap();

    assert!(!ultra.enabled);
    assert_eq!(
        ultra.disabled_reason.as_deref(),
        Some("Codex Ultra is unavailable for the selected model and Codex account.")
    );
}

#[test]
fn disabled_live_capabilities_are_not_selectable_for_workspace_roles() {
    let options = control_options(
        RoutingRole::WorkspaceEdit,
        AgentHarnessKind::Claude,
        Some("sonnet"),
        true,
        &crate::application::agent_capability_gate::AgentCapabilityGate::default(),
    );

    for value in ["rx_native_team", "rx_native_workflow"] {
        let option = options
            .capabilities
            .iter()
            .find(|option| option.value == value)
            .unwrap();
        assert!(
            !option.enabled,
            "{value} must honor the live capability gate"
        );
        assert!(option.disabled_reason.is_some());
    }
}

#[test]
fn unsupported_codex_fast_mode_is_not_selectable_for_role_defaults() {
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
    let options = control_options(
        RoutingRole::WorkspaceEdit,
        AgentHarnessKind::Codex,
        Some("gpt-5.5"),
        true,
        &crate::application::agent_capability_gate::AgentCapabilityGate::default(),
    );
    let fast = options
        .speeds
        .iter()
        .find(|option| option.value == "fast")
        .unwrap();

    assert!(!fast.enabled);
    assert!(fast.disabled_reason.is_some());
}

#[tokio::test]
async fn unsupported_codex_model_is_rejected_before_role_default_persistence() {
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
    let state = AppState::new_test();
    let error = update_manual_role_default_for_state(
        UpdateManualRoleDefaultInput {
            project_id: None,
            role: "workspace_edit".into(),
            value: ManualRoleDefaultInput {
                provider: "codex".into(),
                model: Some("definitely-not-an-ultra-model".into()),
                effort: None,
                service_tier: "provider_default".into(),
                coordination_mode: Some("codex_native_ultra".into()),
                persona_id: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        },
        &state,
    )
    .await
    .expect_err("unsupported Ultra model must fail before persistence");

    assert_eq!(
        error,
        "Codex Ultra is unavailable for the selected model and Codex account."
    );
    assert!(state
        .manual_role_default_repo
        .list_global()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn disabled_team_is_rejected_before_role_default_persistence() {
    let state = AppState::new_test();
    let error = update_manual_role_default_for_state(
        UpdateManualRoleDefaultInput {
            project_id: None,
            role: "workspace_edit".into(),
            value: ManualRoleDefaultInput {
                provider: "claude".into(),
                model: Some("sonnet".into()),
                effort: None,
                service_tier: "provider_default".into(),
                coordination_mode: Some("rx_native_team".into()),
                persona_id: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        },
        &state,
    )
    .await
    .expect_err("disabled Team must fail before persistence");

    assert!(error.contains("Team is disabled"));
    assert!(state
        .manual_role_default_repo
        .list_global()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn unsupported_fast_mode_is_rejected_before_role_default_persistence() {
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
    let state = AppState::new_test();
    let error = update_manual_role_default_for_state(
        UpdateManualRoleDefaultInput {
            project_id: None,
            role: "workspace_edit".into(),
            value: ManualRoleDefaultInput {
                provider: "codex".into(),
                model: Some("gpt-5.5".into()),
                effort: None,
                service_tier: "fast".into(),
                coordination_mode: Some("solo".into()),
                persona_id: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        },
        &state,
    )
    .await
    .expect_err("unsupported Fast mode must fail before persistence");

    assert!(error.contains("Fast mode is not supported"));
    assert!(state
        .manual_role_default_repo
        .list_global()
        .await
        .unwrap()
        .is_empty());
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

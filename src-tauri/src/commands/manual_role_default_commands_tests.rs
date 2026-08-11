use crate::application::manual_role_default_service::{
    ManualDefaultSource, ResolvedManualRoleDefault,
};
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, ManualRoleDefault, ManualServiceTier, RoutingRole};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, CoordinationMode, Project, ProjectId, TaskId,
};

use super::manual_role_default_commands::{
    catalog_entry, control_options, parse_input, reset_agent_conversation_role_default_for_state,
    update_manual_role_default_for_state, ManualRoleDefaultInput,
    ResetAgentConversationRoleDefaultInput, UpdateManualRoleDefaultInput,
};

#[test]
fn catalog_entry_maps_the_backend_owned_role_description() {
    let role = RoutingRole::WorkspaceChat;
    let resolved = ResolvedManualRoleDefault {
        role,
        value: ManualRoleDefault {
            harness: AgentHarnessKind::Claude,
            model: None,
            effort: None,
            service_tier: ManualServiceTier::ProviderDefault,
            coordination_mode: None,
            persona_id: None,
            approval_policy: None,
            sandbox_mode: None,
        },
        source: ManualDefaultSource::ProviderDefault,
        diagnostics: Vec::new(),
    };

    let entry = catalog_entry(
        role,
        None,
        Ok(resolved),
        false,
        &crate::application::agent_capability_gate::AgentCapabilityGate::default(),
    );

    assert_eq!(entry.description, role.metadata().description);
    assert!(!entry.description.trim().is_empty());
}

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

#[test]
fn parses_trimmed_optional_fields_and_drops_blank_values() {
    let value = parse_input(ManualRoleDefaultInput {
        provider: "claude".into(),
        model: Some("  sonnet  ".into()),
        effort: None,
        service_tier: "provider_default".into(),
        coordination_mode: None,
        persona_id: Some("   ".into()),
        approval_policy: Some("  never  ".into()),
        sandbox_mode: Some("  danger-full-access  ".into()),
    })
    .unwrap();

    assert_eq!(value.harness, AgentHarnessKind::Claude);
    assert_eq!(value.model.as_deref(), Some("sonnet"));
    assert_eq!(value.effort, None);
    assert_eq!(value.service_tier, ManualServiceTier::ProviderDefault);
    assert_eq!(value.coordination_mode, None);
    assert_eq!(value.persona_id, None);
    assert_eq!(value.approval_policy.as_deref(), Some("never"));
    assert_eq!(value.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[tokio::test]
async fn global_role_default_update_persists_and_returns_normalized_value() {
    let state = AppState::new_test();

    let response = update_manual_role_default_for_state(
        UpdateManualRoleDefaultInput {
            project_id: None,
            role: "workspace_chat".into(),
            value: ManualRoleDefaultInput {
                provider: "claude".into(),
                model: Some("  sonnet  ".into()),
                effort: Some("medium".into()),
                service_tier: "standard".into(),
                coordination_mode: Some("solo".into()),
                persona_id: None,
                approval_policy: Some("  never  ".into()),
                sandbox_mode: Some("  danger-full-access  ".into()),
            },
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(response.provider, "claude");
    assert_eq!(response.model.as_deref(), Some("sonnet"));
    assert_eq!(response.effort.as_deref(), Some("medium"));
    assert_eq!(response.service_tier, "standard");
    assert_eq!(response.coordination_mode.as_deref(), Some("solo"));
    assert_eq!(response.approval_policy.as_deref(), Some("never"));
    assert_eq!(response.sandbox_mode.as_deref(), Some("danger-full-access"));

    let stored = state
        .manual_role_default_repo
        .get_global(RoutingRole::WorkspaceChat)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.value.harness, AgentHarnessKind::Claude);
    assert_eq!(stored.value.model.as_deref(), Some("sonnet"));
    assert_eq!(stored.value.effort.unwrap().to_string(), "medium");
    assert_eq!(stored.value.service_tier, ManualServiceTier::Standard);
    assert_eq!(
        stored.value.coordination_mode.unwrap(),
        CoordinationMode::Solo
    );
    assert_eq!(stored.value.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        stored.value.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );
}

#[tokio::test]
async fn project_role_default_update_persists_without_touching_global_defaults() {
    let state = AppState::new_test();
    let project_id = "project-role-defaults";

    let response = update_manual_role_default_for_state(
        UpdateManualRoleDefaultInput {
            project_id: Some(project_id.into()),
            role: "workspace_plan".into(),
            value: ManualRoleDefaultInput {
                provider: "claude".into(),
                model: None,
                effort: None,
                service_tier: "provider_default".into(),
                coordination_mode: None,
                persona_id: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(response.provider, "claude");
    assert_eq!(response.service_tier, "provider_default");
    assert_eq!(response.model, None);
    assert_eq!(response.coordination_mode, None);

    let project_row = state
        .manual_role_default_repo
        .get_for_project(project_id, RoutingRole::WorkspacePlan)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(project_row.value.harness, AgentHarnessKind::Claude);
    assert_eq!(
        project_row.value.service_tier,
        ManualServiceTier::ProviderDefault
    );
    assert!(state
        .manual_role_default_repo
        .get_global(RoutingRole::WorkspacePlan)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn invalid_update_role_is_rejected_before_persistence() {
    let state = AppState::new_test();

    let error = update_manual_role_default_for_state(
        UpdateManualRoleDefaultInput {
            project_id: None,
            role: "not_a_role".into(),
            value: ManualRoleDefaultInput {
                provider: "claude".into(),
                model: None,
                effort: None,
                service_tier: "provider_default".into(),
                coordination_mode: None,
                persona_id: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        },
        &state,
    )
    .await
    .unwrap_err();

    assert!(error.contains("Invalid routing role"));
    assert!(state
        .manual_role_default_repo
        .list_global()
        .await
        .unwrap()
        .is_empty());
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
async fn reset_rejects_missing_conversation_without_persistence_side_effects() {
    let state = AppState::new_test();

    let error = reset_agent_conversation_role_default_for_state(
        ResetAgentConversationRoleDefaultInput {
            conversation_id: "missing-conversation".into(),
        },
        &state,
        None,
    )
    .await
    .unwrap_err();

    assert!(error.contains("Conversation not found"));
    assert!(state
        .manual_role_default_repo
        .list_global()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn reset_rejects_non_project_conversation_before_role_resolution() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_task(TaskId::from_string(
            "task-1".into(),
        )))
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

    assert_eq!(
        error,
        "Role-default reset is available only for project conversations"
    );
}

#[tokio::test]
async fn reset_rejects_project_conversation_when_project_is_missing() {
    let state = AppState::new_test();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("missing-project".into()));
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Plan));
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

    assert!(error.contains("Project not found: missing-project"));
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

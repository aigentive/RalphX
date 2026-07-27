use std::fs;
use std::sync::Arc;

use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, AgentProviderSettings, ManualRoleDefault,
    ManualServiceTier, RoutingRole,
};
use crate::domain::entities::CoordinationMode;
use crate::domain::entities::PersonaId;
use crate::domain::repositories::{
    AgentLaneSettingsRepository, AgentProviderSettingsRepository, ManualRoleDefaultRepository,
};
use crate::infrastructure::memory::{
    MemoryAgentLaneSettingsRepository, MemoryAgentProviderSettingsRepository,
    MemoryManualRoleDefaultRepository, MemoryPersonaRepository,
};

use super::manual_role_default_service::{ManualDefaultSource, ManualRoleDefaultService};

fn exact(model: &str) -> ManualRoleDefault {
    ManualRoleDefault {
        harness: AgentHarnessKind::Codex,
        model: Some(model.to_string()),
        effort: None,
        service_tier: ManualServiceTier::Standard,
        coordination_mode: None,
        persona_id: None,
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
    }
}

fn service(
    global_router_path: std::path::PathBuf,
) -> (
    ManualRoleDefaultService,
    Arc<MemoryManualRoleDefaultRepository>,
    Arc<MemoryAgentLaneSettingsRepository>,
) {
    let manual_repo = Arc::new(MemoryManualRoleDefaultRepository::new());
    let lane_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    let lane_repo_trait: Arc<dyn AgentLaneSettingsRepository> = lane_repo.clone();
    let provider_repo: Arc<dyn AgentProviderSettingsRepository> = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Claude),
    );
    let service = ManualRoleDefaultService::new(
        manual_repo.clone(),
        lane_repo_trait,
        provider_repo,
        Arc::new(MemoryPersonaRepository::new()),
        Arc::new(crate::application::agent_capability_gate::AgentCapabilityGate::default()),
        true,
        global_router_path,
    );
    (service, manual_repo, lane_repo)
}

#[tokio::test]
async fn resolves_whole_value_precedence_project_ui_over_yaml_over_global_ui() {
    let global_root = tempfile::tempdir().unwrap();
    let global_path = global_root.path().join("router.yaml");
    fs::write(
        &global_path,
        "manual:\n  defaults:\n    roles:\n      workspace_edit:\n        provider: codex\n        model: global-yaml\n",
    )
    .unwrap();
    let project_root = tempfile::tempdir().unwrap();
    fs::create_dir_all(project_root.path().join(".ralphx")).unwrap();
    fs::write(
        project_root.path().join(".ralphx/router.yaml"),
        "manual:\n  defaults:\n    roles:\n      workspace_edit:\n        provider: codex\n        model: project-yaml\n",
    )
    .unwrap();
    let (service, repo, _lane_repo) = service(global_path);
    repo.upsert_global(RoutingRole::WorkspaceEdit, &exact("global-ui"))
        .await
        .unwrap();

    let from_yaml = service
        .resolve(
            Some("project-1"),
            Some(project_root.path()),
            RoutingRole::WorkspaceEdit,
        )
        .await
        .unwrap();
    assert_eq!(from_yaml.source, ManualDefaultSource::ProjectYaml);
    assert_eq!(from_yaml.value.model.as_deref(), Some("project-yaml"));

    repo.upsert_for_project(
        "project-1",
        RoutingRole::WorkspaceEdit,
        &exact("project-ui"),
    )
    .await
    .unwrap();
    let from_ui = service
        .resolve(
            Some("project-1"),
            Some(project_root.path()),
            RoutingRole::WorkspaceEdit,
        )
        .await
        .unwrap();
    assert_eq!(from_ui.source, ManualDefaultSource::ProjectUi);
    assert_eq!(from_ui.value.model.as_deref(), Some("project-ui"));
}

#[tokio::test]
async fn legacy_lane_is_used_only_after_explicit_sources_are_absent() {
    let global_root = tempfile::tempdir().unwrap();
    let (service, _repo, lane_repo) = service(global_root.path().join("router.yaml"));
    lane_repo
        .upsert_global(
            AgentLane::ExecutionBranchUpdater,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Codex,
                model: Some("legacy-repair".to_string()),
                effort: None,
                approval_policy: Some("never".to_string()),
                sandbox_mode: Some("danger-full-access".to_string()),
            },
        )
        .await
        .unwrap();

    let resolved = service
        .resolve(None, None, RoutingRole::WorkspaceRepair)
        .await
        .unwrap();
    assert_eq!(resolved.source, ManualDefaultSource::LegacyLane);
    assert_eq!(resolved.value.model.as_deref(), Some("legacy-repair"));
}

#[tokio::test]
async fn global_ui_default_wins_over_global_yaml_and_provider_default() {
    let global_root = tempfile::tempdir().unwrap();
    let global_path = global_root.path().join("router.yaml");
    fs::write(
        &global_path,
        "manual:\n  defaults:\n    roles:\n      workspace_chat:\n        provider: codex\n        model: global-yaml\n",
    )
    .unwrap();
    let (service, repo, _lane_repo) = service(global_path);
    repo.upsert_global(RoutingRole::WorkspaceChat, &exact("global-ui"))
        .await
        .unwrap();

    let resolved = service
        .resolve(None, None, RoutingRole::WorkspaceChat)
        .await
        .unwrap();

    assert_eq!(resolved.source, ManualDefaultSource::GlobalUi);
    assert_eq!(resolved.source.key(), "global_ui");
    assert_eq!(resolved.value.model.as_deref(), Some("global-ui"));
}

#[tokio::test]
async fn global_yaml_default_is_used_when_ui_defaults_are_absent() {
    let global_root = tempfile::tempdir().unwrap();
    let global_path = global_root.path().join("router.yaml");
    fs::write(
        &global_path,
        "manual:\n  defaults:\n    roles:\n      workspace_chat:\n        provider: codex\n        model: global-yaml\n        service_tier: standard\n",
    )
    .unwrap();
    let (service, _repo, _lane_repo) = service(global_path);

    let resolved = service
        .resolve(None, None, RoutingRole::WorkspaceChat)
        .await
        .unwrap();

    assert_eq!(resolved.source, ManualDefaultSource::GlobalYaml);
    assert_eq!(resolved.source.key(), "global_yaml");
    assert_eq!(resolved.value.model.as_deref(), Some("global-yaml"));
    assert_eq!(resolved.value.service_tier, ManualServiceTier::Standard);
}

#[tokio::test]
async fn provider_default_preserves_service_tier_permissions_and_sandbox() {
    let global_root = tempfile::tempdir().unwrap();
    let manual_repo = Arc::new(MemoryManualRoleDefaultRepository::new());
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());
    let provider_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut provider = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    provider.enabled = true;
    provider.is_default = true;
    provider.model = Some("gpt-5.6".to_string());
    provider.service_tier = Some("standard".to_string());
    provider.approval_policy = Some("never".to_string());
    provider.sandbox_mode = Some("danger-full-access".to_string());
    provider_repo.upsert(&provider).await.unwrap();
    let service = ManualRoleDefaultService::new(
        manual_repo,
        lane_repo,
        provider_repo,
        Arc::new(MemoryPersonaRepository::new()),
        Arc::new(crate::application::agent_capability_gate::AgentCapabilityGate::default()),
        true,
        global_root.path().join("router.yaml"),
    );

    let resolved = service
        .resolve(None, None, RoutingRole::WorkspaceChat)
        .await
        .unwrap();

    assert_eq!(resolved.source, ManualDefaultSource::ProviderDefault);
    assert_eq!(resolved.source.key(), "provider_default");
    assert_eq!(resolved.value.model.as_deref(), Some("gpt-5.6"));
    assert_eq!(resolved.value.service_tier, ManualServiceTier::Standard);
    assert_eq!(resolved.value.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        resolved.value.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );
}

#[tokio::test]
async fn persona_feature_off_suppresses_validation_without_discarding_the_default() {
    let global_root = tempfile::tempdir().unwrap();
    let manual_repo = Arc::new(MemoryManualRoleDefaultRepository::new());
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());
    let provider_repo: Arc<dyn AgentProviderSettingsRepository> = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Claude),
    );
    let service = ManualRoleDefaultService::new(
        manual_repo.clone(),
        lane_repo,
        provider_repo,
        Arc::new(MemoryPersonaRepository::new()),
        Arc::new(crate::application::agent_capability_gate::AgentCapabilityGate::default()),
        false,
        global_root.path().join("router.yaml"),
    );
    let mut value = exact("persona-default");
    value.persona_id = Some(PersonaId::from_string("persona-1"));
    manual_repo
        .upsert_global(RoutingRole::WorkspaceEdit, &value)
        .await
        .unwrap();

    let resolved = service
        .resolve(None, None, RoutingRole::WorkspaceEdit)
        .await
        .unwrap();

    assert_eq!(resolved.value.persona_id, value.persona_id);
}

#[tokio::test]
async fn resolution_fails_closed_when_a_stored_team_default_is_disabled() {
    let global_root = tempfile::tempdir().unwrap();
    let (service, repo, _lane_repo) = service(global_root.path().join("router.yaml"));
    let mut value = exact("team-default");
    value.coordination_mode = Some(CoordinationMode::RxNativeTeam);
    repo.upsert_global(RoutingRole::WorkspaceEdit, &value)
        .await
        .unwrap();

    let error = service
        .resolve(None, None, RoutingRole::WorkspaceEdit)
        .await
        .expect_err("disabled stored Team default must fail closed");

    assert!(error.to_string().contains("Team is disabled"));
}

#[tokio::test]
async fn resolution_fails_closed_when_stored_fast_mode_is_unsupported() {
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
    let global_root = tempfile::tempdir().unwrap();
    let (service, repo, _lane_repo) = service(global_root.path().join("router.yaml"));
    let mut value = exact("gpt-5.5");
    value.service_tier = ManualServiceTier::Fast;
    repo.upsert_global(RoutingRole::WorkspaceEdit, &value)
        .await
        .unwrap();

    let error = service
        .resolve(None, None, RoutingRole::WorkspaceEdit)
        .await
        .expect_err("unsupported stored Fast default must fail closed");

    assert!(error.to_string().contains("Fast mode is not supported"));
}

#[tokio::test]
async fn resolution_rejects_missing_persona_when_personas_are_enabled() {
    let global_root = tempfile::tempdir().unwrap();
    let (service, repo, _lane_repo) = service(global_root.path().join("router.yaml"));
    let mut value = exact("persona-default");
    value.persona_id = Some(PersonaId::from_string("missing-persona".to_string()));
    repo.upsert_global(RoutingRole::WorkspaceEdit, &value)
        .await
        .unwrap();

    let error = service
        .resolve(None, None, RoutingRole::WorkspaceEdit)
        .await
        .expect_err("missing persona must fail closed");

    assert!(error.to_string().contains("Persona not found"));
}

#[test]
fn validate_role_value_rejects_unsupported_manual_controls() {
    let mut value = exact("unsupported");
    value.coordination_mode = Some(CoordinationMode::CodexNativeUltra);
    value.harness = AgentHarnessKind::Claude;
    assert!(super::manual_role_default_service::validate_role_value(
        RoutingRole::WorkspaceEdit,
        &value
    )
    .unwrap_err()
    .to_string()
    .contains("Codex Ultra requires"));

    value.coordination_mode = Some(CoordinationMode::RxNativeWorkflow);
    value.harness = AgentHarnessKind::Codex;
    assert!(super::manual_role_default_service::validate_role_value(
        RoutingRole::ExecutionWorker,
        &value
    )
    .unwrap_err()
    .to_string()
    .contains("Capability is not supported"));

    value.coordination_mode = None;
    value.persona_id = Some(PersonaId::from_string("persona-1".to_string()));
    assert!(super::manual_role_default_service::validate_role_value(
        RoutingRole::ExecutionWorker,
        &value
    )
    .unwrap_err()
    .to_string()
    .contains("Persona is not supported"));
}

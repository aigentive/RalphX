use std::fs;
use std::sync::Arc;

use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, ManualRoleDefault, ManualServiceTier,
    RoutingRole,
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

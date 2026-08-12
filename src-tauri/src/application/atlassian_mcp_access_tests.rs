use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::application::agent_capability_gate::AgentCapabilityGate;
use crate::application::atlassian_integration_service::{
    AtlassianIntegrationService, EmptyAtlassianApiClient,
};
use crate::application::manual_role_default_service::ManualRoleDefaultService;
use crate::application::{atlassian_mcp_tools_for_spawn, effective_atlassian_mcp_access};
use crate::domain::agents::{AgentHarnessKind, AtlassianMcpAccess, RoutingRole};
use crate::domain::integrations::{
    AtlassianAuthMethod, AtlassianIntegrationSettings, AtlassianIntegrationSettingsRepository,
    IntegrationValidationStatus,
};
use crate::domain::repositories::{AgentLaneSettingsRepository, ManualRoleDefaultRepository};
use crate::infrastructure::memory::{
    MemoryAgentLaneSettingsRepository, MemoryAgentProviderSettingsRepository,
    MemoryManualRoleDefaultRepository, MemoryPersonaRepository, MemorySecretStore,
};

/// Settings repository whose `get()` can be pointed at any state, including a
/// hard failure, so fail-closed behavior is provable.
struct FakeSettingsRepo {
    settings: RwLock<AtlassianIntegrationSettings>,
    fail: bool,
}

impl FakeSettingsRepo {
    fn with(enabled: bool, validation_status: IntegrationValidationStatus) -> Self {
        Self {
            settings: RwLock::new(AtlassianIntegrationSettings {
                enabled,
                auth_method: AtlassianAuthMethod::ApiToken,
                site_url: Some("https://example.atlassian.net".to_string()),
                email: Some("user@example.com".to_string()),
                validation_status,
                ..AtlassianIntegrationSettings::default()
            }),
            fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            settings: RwLock::new(AtlassianIntegrationSettings::default()),
            fail: true,
        }
    }
}

#[async_trait]
impl AtlassianIntegrationSettingsRepository for FakeSettingsRepo {
    async fn get(&self) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>> {
        if self.fail {
            return Err("settings unavailable".into());
        }
        Ok(self.settings.read().await.clone())
    }

    async fn upsert(
        &self,
        settings: &AtlassianIntegrationSettings,
    ) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>> {
        *self.settings.write().await = settings.clone();
        Ok(settings.clone())
    }
}

fn integration(repo: FakeSettingsRepo) -> AtlassianIntegrationService {
    AtlassianIntegrationService::new(
        Arc::new(repo),
        Arc::new(MemorySecretStore::new()),
        Arc::new(EmptyAtlassianApiClient),
    )
}

fn usable_integration() -> AtlassianIntegrationService {
    integration(FakeSettingsRepo::with(
        true,
        IntegrationValidationStatus::Valid,
    ))
}

fn role_defaults() -> (
    ManualRoleDefaultService,
    Arc<MemoryManualRoleDefaultRepository>,
) {
    let manual_repo = Arc::new(MemoryManualRoleDefaultRepository::new());
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());
    let service = ManualRoleDefaultService::new(
        manual_repo.clone(),
        lane_repo,
        Arc::new(
            MemoryAgentProviderSettingsRepository::with_all_providers_enabled(
                AgentHarnessKind::Claude,
            ),
        ),
        Arc::new(MemoryPersonaRepository::new()),
        Arc::new(AgentCapabilityGate::default()),
        true,
        std::path::PathBuf::from("/nonexistent/router.yaml"),
    );
    (service, manual_repo)
}

#[tokio::test]
async fn a_usable_integration_grants_the_built_in_role_tier() {
    let integration = usable_integration();
    let (defaults, _repo) = role_defaults();

    assert_eq!(
        effective_atlassian_mcp_access(
            &integration,
            &defaults,
            Some(RoutingRole::WorkspaceEdit),
            None,
            None
        )
        .await,
        AtlassianMcpAccess::ReadWrite
    );
    assert_eq!(
        effective_atlassian_mcp_access(
            &integration,
            &defaults,
            Some(RoutingRole::WorkspaceReviewer),
            None,
            None
        )
        .await,
        AtlassianMcpAccess::Read
    );
}

#[tokio::test]
async fn a_disabled_integration_denies_every_role() {
    let integration = integration(FakeSettingsRepo::with(
        false,
        IntegrationValidationStatus::Valid,
    ));
    let (defaults, _repo) = role_defaults();

    for role in [RoutingRole::WorkspaceEdit, RoutingRole::WorkspaceReviewer] {
        assert_eq!(
            effective_atlassian_mcp_access(&integration, &defaults, Some(role), None, None).await,
            AtlassianMcpAccess::None,
            "{role} must be denied while the integration is disabled"
        );
    }
}

#[tokio::test]
async fn an_enabled_but_unvalidated_integration_denies_every_role() {
    // `enabled` alone is not the gate: the integration must also be Valid.
    for status in [
        IntegrationValidationStatus::NotConfigured,
        IntegrationValidationStatus::Pending,
        IntegrationValidationStatus::Invalid,
    ] {
        let service = integration(FakeSettingsRepo::with(true, status.clone()));
        let (defaults, _repo) = role_defaults();

        assert_eq!(
            effective_atlassian_mcp_access(
                &service,
                &defaults,
                Some(RoutingRole::WorkspaceEdit),
                None,
                None
            )
            .await,
            AtlassianMcpAccess::None,
            "validation_status {status:?} must deny"
        );
    }
}

#[tokio::test]
async fn a_settings_read_failure_denies_rather_than_defaulting_open() {
    let integration = integration(FakeSettingsRepo::failing());
    let (defaults, _repo) = role_defaults();

    assert_eq!(
        effective_atlassian_mcp_access(
            &integration,
            &defaults,
            Some(RoutingRole::WorkspaceEdit),
            None,
            None
        )
        .await,
        AtlassianMcpAccess::None
    );
}

#[tokio::test]
async fn an_absent_routing_role_denies_without_consulting_the_integration() {
    let integration = usable_integration();
    let (defaults, _repo) = role_defaults();

    assert_eq!(
        effective_atlassian_mcp_access(&integration, &defaults, None, Some("project-1"), None)
            .await,
        AtlassianMcpAccess::None
    );
}

#[tokio::test]
async fn a_role_override_of_none_removes_access_even_when_the_integration_is_usable() {
    let integration = usable_integration();
    let (defaults, repo) = role_defaults();
    let mut value = crate::domain::agents::ManualRoleDefault {
        harness: AgentHarnessKind::Claude,
        model: None,
        effort: None,
        service_tier: crate::domain::agents::ManualServiceTier::ProviderDefault,
        coordination_mode: None,
        persona_id: None,
        approval_policy: None,
        sandbox_mode: None,
        atlassian_access: None,
    };
    value.atlassian_access = Some(AtlassianMcpAccess::None);
    repo.upsert_for_project("project-1", RoutingRole::WorkspaceEdit, &value)
        .await
        .unwrap();

    assert_eq!(
        effective_atlassian_mcp_access(
            &integration,
            &defaults,
            Some(RoutingRole::WorkspaceEdit),
            Some("project-1"),
            None
        )
        .await,
        AtlassianMcpAccess::None
    );
}

#[tokio::test]
async fn spawn_tool_names_follow_the_effective_tier() {
    let usable = usable_integration();
    let (defaults, _repo) = role_defaults();

    let write_tools = atlassian_mcp_tools_for_spawn(
        &usable,
        &defaults,
        Some(RoutingRole::WorkspaceEdit),
        None,
        None,
    )
    .await;
    assert!(write_tools.contains(&"jira_create_issue".to_string()));
    assert!(write_tools.contains(&"jira_search_issues".to_string()));

    let read_tools = atlassian_mcp_tools_for_spawn(
        &usable,
        &defaults,
        Some(RoutingRole::WorkspaceReviewer),
        None,
        None,
    )
    .await;
    assert!(read_tools.contains(&"jira_search_issues".to_string()));
    assert!(!read_tools.contains(&"jira_create_issue".to_string()));

    let disabled = integration(FakeSettingsRepo::with(
        false,
        IntegrationValidationStatus::Valid,
    ));
    let denied = atlassian_mcp_tools_for_spawn(
        &disabled,
        &defaults,
        Some(RoutingRole::WorkspaceEdit),
        None,
        None,
    )
    .await;
    assert!(
        denied.is_empty(),
        "disabled integration must inject nothing"
    );
}

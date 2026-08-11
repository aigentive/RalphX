use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::domain::agents::{
    default_atlassian_access, AgentLane, AgentProviderSettings, AtlassianMcpAccess,
    ManualRoleDefault, ManualServiceTier, RoutingRole, RoutingRoleFamily, DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::{CoordinationMode, PersonaStatus};
use crate::domain::repositories::{
    AgentLaneSettingsRepository, AgentProviderSettingsRepository, ManualRoleDefaultRepository,
    PersonaRepository,
};
use crate::error::{AppError, AppResult};

use super::agent_capability_gate::AgentCapabilityGate;
use super::manual_router_config::{load_manual_router_file, ManualRouterSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualDefaultSource {
    ProjectUi,
    ProjectYaml,
    GlobalUi,
    GlobalYaml,
    LegacyLane,
    LegacyWorkspaceReview,
    ProviderDefault,
}

impl ManualDefaultSource {
    pub const fn key(self) -> &'static str {
        match self {
            Self::ProjectUi => "project_ui",
            Self::ProjectYaml => "project_yaml",
            Self::GlobalUi => "global_ui",
            Self::GlobalYaml => "global_yaml",
            Self::LegacyLane => "legacy_lane",
            Self::LegacyWorkspaceReview => "legacy_workspace_review",
            Self::ProviderDefault => "provider_default",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedManualRoleDefault {
    pub role: RoutingRole,
    pub value: ManualRoleDefault,
    pub source: ManualDefaultSource,
    pub diagnostics: Vec<String>,
}

#[derive(Clone)]
pub struct ManualRoleDefaultService {
    manual_repo: Arc<dyn ManualRoleDefaultRepository>,
    lane_repo: Arc<dyn AgentLaneSettingsRepository>,
    provider_repo: Arc<dyn AgentProviderSettingsRepository>,
    persona_repo: Arc<dyn PersonaRepository>,
    capability_gate: Arc<AgentCapabilityGate>,
    personas_enabled: bool,
    global_router_path: PathBuf,
}

impl ManualRoleDefaultService {
    pub fn new(
        manual_repo: Arc<dyn ManualRoleDefaultRepository>,
        lane_repo: Arc<dyn AgentLaneSettingsRepository>,
        provider_repo: Arc<dyn AgentProviderSettingsRepository>,
        persona_repo: Arc<dyn PersonaRepository>,
        capability_gate: Arc<AgentCapabilityGate>,
        personas_enabled: bool,
        global_router_path: PathBuf,
    ) -> Self {
        Self {
            manual_repo,
            lane_repo,
            provider_repo,
            persona_repo,
            capability_gate,
            personas_enabled,
            global_router_path,
        }
    }

    pub async fn resolve(
        &self,
        project_id: Option<&str>,
        project_root: Option<&Path>,
        role: RoutingRole,
    ) -> AppResult<ResolvedManualRoleDefault> {
        let resolved = self
            .resolve_unvalidated(project_id, project_root, role)
            .await?;
        self.validate_value(role, &resolved.value).await?;
        Ok(resolved)
    }

    /// Resolve the configured role entry without validating runtime fields that
    /// a complete per-launch override replaces. The caller must validate the
    /// materialized explicit runtime before spawning.
    pub async fn resolve_for_explicit_runtime(
        &self,
        project_id: Option<&str>,
        project_root: Option<&Path>,
        role: RoutingRole,
    ) -> AppResult<ResolvedManualRoleDefault> {
        self.resolve_unvalidated(project_id, project_root, role)
            .await
    }

    async fn resolve_unvalidated(
        &self,
        project_id: Option<&str>,
        project_root: Option<&Path>,
        role: RoutingRole,
    ) -> AppResult<ResolvedManualRoleDefault> {
        if let Some(project_id) = project_id {
            if let Some(row) = self
                .manual_repo
                .get_for_project(project_id, role)
                .await
                .map_err(|error| repository_error("project manual role default", error))?
            {
                return Ok(resolved(role, row.value, ManualDefaultSource::ProjectUi));
            }
        }

        if let Some(project_root) = project_root {
            let project_router = project_root.join(".ralphx").join("router.yaml");
            if let Some(resolved) = resolve_yaml(
                role,
                load_manual_router_file(project_root, &project_router)?,
                ManualDefaultSource::ProjectYaml,
            )? {
                return Ok(resolved);
            }
        }

        if let Some(row) = self
            .manual_repo
            .get_global(role)
            .await
            .map_err(|error| repository_error("global manual role default", error))?
        {
            return Ok(resolved(role, row.value, ManualDefaultSource::GlobalUi));
        }

        let global_root = self.global_router_path.parent().ok_or_else(|| {
            AppError::Infrastructure("Global router path has no config root".into())
        })?;
        if let Some(resolved) = resolve_yaml(
            role,
            load_manual_router_file(global_root, &self.global_router_path)?,
            ManualDefaultSource::GlobalYaml,
        )? {
            return Ok(resolved);
        }

        if let Some(lane) = legacy_lane_for_role_default(role) {
            let project_row = match project_id {
                Some(project_id) => self
                    .lane_repo
                    .get_for_project(project_id, lane)
                    .await
                    .map_err(|error| repository_error("project legacy lane default", error))?,
                None => None,
            };
            let global_row = self
                .lane_repo
                .get_global(lane)
                .await
                .map_err(|error| repository_error("global legacy lane default", error))?;
            if let Some(row) = project_row.or(global_row) {
                let value = ManualRoleDefault::from_legacy(&row.settings);
                return Ok(resolved(role, value, ManualDefaultSource::LegacyLane));
            }
        }

        let provider = self
            .provider_repo
            .get_default()
            .await
            .map_err(|error| repository_error("default provider settings", error))?
            .unwrap_or_else(|| AgentProviderSettings::disabled_defaults(DEFAULT_AGENT_HARNESS));
        let value = provider_default(provider);
        Ok(resolved(role, value, ManualDefaultSource::ProviderDefault))
    }

    async fn validate_value(&self, role: RoutingRole, value: &ManualRoleDefault) -> AppResult<()> {
        validate_role_value(role, value)?;
        crate::application::agent_capability_validation::validate_manual_role_runtime_capabilities(
            value,
            &self.capability_gate,
        )
        .map_err(AppError::Validation)?;
        let Some(persona_id) = value.persona_id.as_ref() else {
            return Ok(());
        };
        if !self.personas_enabled {
            return Ok(());
        }
        let persona = self
            .persona_repo
            .get_by_id(persona_id)
            .await?
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "Persona not found for manual role default: {persona_id}"
                ))
            })?;
        if persona.status != PersonaStatus::Active {
            return Err(AppError::Validation(format!(
                "Persona is not active for manual role default: {persona_id}"
            )));
        }
        Ok(())
    }

    /// Validate a complete explicit runtime value against the same role,
    /// provider, capability, and persona rules used by persisted defaults.
    pub async fn validate_explicit_value(
        &self,
        role: RoutingRole,
        value: &ManualRoleDefault,
    ) -> AppResult<()> {
        self.validate_value(role, value).await
    }

    /// Resolve the Atlassian MCP tier configured for a routing role, before the
    /// integration-enablement gate.
    ///
    /// Resolution is row-wins, matching every other field on
    /// [`ManualRoleDefault`]: the first matching layer supplies the whole row, so
    /// a project row that omits `atlassian_access` falls back to the built-in
    /// [`default_atlassian_access`] tier rather than to the global row's value.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying role default cannot be resolved.
    pub async fn resolve_atlassian_access(
        &self,
        project_id: Option<&str>,
        project_root: Option<&Path>,
        role: RoutingRole,
    ) -> AppResult<AtlassianMcpAccess> {
        let resolved = self
            .resolve_unvalidated(project_id, project_root, role)
            .await?;
        Ok(resolved
            .value
            .atlassian_access
            .unwrap_or_else(|| default_atlassian_access(role)))
    }

    /// Resolve the enabled provider settings used by an explicit launch selection.
    pub async fn resolve_enabled_provider_settings(
        &self,
        harness: crate::domain::agents::AgentHarnessKind,
        surface_name: &str,
    ) -> AppResult<AgentProviderSettings> {
        crate::application::ensure_provider_spawn_enabled(
            &self.provider_repo,
            harness,
            surface_name,
        )
        .await
        .map_err(AppError::Validation)?;
        self.provider_repo
            .get(harness)
            .await
            .map_err(|error| repository_error("selected provider settings", error))?
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "{surface_name}: {harness} is not configured in Settings > Harness > Providers."
                ))
            })
    }
}

fn legacy_lane_for_role_default(role: RoutingRole) -> Option<AgentLane> {
    role.legacy_lane().or(match role {
        RoutingRole::ExecutionQaPrep
        | RoutingRole::ExecutionQaRefiner
        | RoutingRole::ExecutionQaTester => Some(AgentLane::ExecutionWorker),
        RoutingRole::UtilityLightweight => Some(AgentLane::IdeationPrimary),
        _ => None,
    })
}

fn resolve_yaml(
    role: RoutingRole,
    mut snapshot: ManualRouterSnapshot,
    source: ManualDefaultSource,
) -> AppResult<Option<ResolvedManualRoleDefault>> {
    let Some(entry) = snapshot.entries.remove(&role) else {
        return Ok(None);
    };
    let value = entry.map_err(|diagnostic| {
        AppError::Validation(format!("Invalid {source:?} for {role}: {diagnostic}"))
    })?;
    validate_role_value(role, &value)?;
    Ok(Some(ResolvedManualRoleDefault {
        role,
        value,
        source,
        diagnostics: snapshot.diagnostics,
    }))
}

fn resolved(
    role: RoutingRole,
    value: ManualRoleDefault,
    source: ManualDefaultSource,
) -> ResolvedManualRoleDefault {
    ResolvedManualRoleDefault {
        role,
        value,
        source,
        diagnostics: Vec::new(),
    }
}

fn repository_error(context: &str, error: impl std::fmt::Display) -> AppError {
    AppError::Infrastructure(format!("Failed to read {context}: {error}"))
}

fn provider_default(provider: AgentProviderSettings) -> ManualRoleDefault {
    let service_tier = match provider.service_tier.as_deref() {
        Some("standard") => ManualServiceTier::Standard,
        Some("fast") => ManualServiceTier::Fast,
        _ => ManualServiceTier::ProviderDefault,
    };
    ManualRoleDefault {
        harness: provider.provider,
        model: provider.model,
        effort: provider.effort,
        service_tier,
        coordination_mode: None,
        persona_id: None,
        approval_policy: provider.approval_policy,
        sandbox_mode: provider.sandbox_mode,
        atlassian_access: None,
    }
}

pub fn validate_role_value(role: RoutingRole, value: &ManualRoleDefault) -> AppResult<()> {
    if value.service_tier == ManualServiceTier::Fast
        && value.harness != crate::domain::agents::AgentHarnessKind::Codex
    {
        return Err(AppError::Validation(
            "Fast speed requires the Codex provider".into(),
        ));
    }

    let is_workspace_root = role.metadata().family == RoutingRoleFamily::Workspace;
    if !is_workspace_root
        && value
            .coordination_mode
            .is_some_and(|mode| mode != CoordinationMode::Solo)
    {
        return Err(AppError::Validation(format!(
            "Capability is not supported for routing role {role}"
        )));
    }
    if value.coordination_mode == Some(CoordinationMode::CodexNativeUltra)
        && value.harness != crate::domain::agents::AgentHarnessKind::Codex
    {
        return Err(AppError::Validation(
            "Codex Ultra requires the Codex provider".into(),
        ));
    }
    if value.persona_id.is_some() && !is_workspace_root {
        return Err(AppError::Validation(format!(
            "Persona is not supported for routing role {role}"
        )));
    }
    Ok(())
}

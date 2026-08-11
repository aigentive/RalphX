use std::sync::Arc;

use crate::application::harness_runtime_registry::default_agent_harness_settings_config;
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, LogicalEffort, StoredAgentLaneSettings,
};
use crate::domain::ideation::{
    EffortLevel, IdeationEffortSettings, IdeationModelSettings, ModelLevel,
};
use crate::domain::repositories::{
    AgentLaneSettingsRepository, IdeationEffortSettingsRepository, IdeationModelSettingsRepository,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedConfiguredLaneSettings {
    pub configured_harness: Option<AgentHarnessKind>,
    pub settings: Option<AgentLaneSettings>,
}

pub(crate) async fn resolve_configured_lane_settings(
    lane: AgentLane,
    project_id: Option<&str>,
    agent_lane_settings_repo: Option<&Arc<dyn AgentLaneSettingsRepository>>,
    ideation_effort_settings_repo: Option<&Arc<dyn IdeationEffortSettingsRepository>>,
    ideation_model_settings_repo: Option<&Arc<dyn IdeationModelSettingsRepository>>,
) -> ResolvedConfiguredLaneSettings {
    let (project_row, global_row) =
        load_lane_rows(agent_lane_settings_repo, project_id, lane).await;
    let (legacy_project_settings, legacy_global_settings) = load_legacy_ideation_lane_settings(
        lane,
        project_id,
        ideation_effort_settings_repo,
        ideation_model_settings_repo,
    )
    .await;

    if let Some(project_row) = project_row.as_ref() {
        return ResolvedConfiguredLaneSettings {
            configured_harness: Some(project_row.settings.harness),
            settings: Some(project_row.settings.clone()),
        };
    }

    if let Some(settings) = legacy_project_settings {
        return ResolvedConfiguredLaneSettings {
            configured_harness: Some(settings.harness),
            settings: Some(settings),
        };
    }

    if should_prefer_legacy_global_settings(
        lane,
        global_row.as_ref(),
        legacy_global_settings.as_ref(),
    ) {
        return ResolvedConfiguredLaneSettings {
            configured_harness: legacy_global_settings
                .as_ref()
                .map(|settings| settings.harness),
            settings: legacy_global_settings,
        };
    }

    if let Some(global_row) = global_row.as_ref() {
        return ResolvedConfiguredLaneSettings {
            configured_harness: Some(global_row.settings.harness),
            settings: Some(global_row.settings.clone()),
        };
    }

    ResolvedConfiguredLaneSettings {
        configured_harness: legacy_global_settings
            .as_ref()
            .map(|settings| settings.harness),
        settings: legacy_global_settings,
    }
}

async fn load_lane_rows(
    repo: Option<&Arc<dyn AgentLaneSettingsRepository>>,
    project_id: Option<&str>,
    lane: AgentLane,
) -> (
    Option<StoredAgentLaneSettings>,
    Option<StoredAgentLaneSettings>,
) {
    let Some(repo) = repo else {
        return (None, None);
    };

    let project_row = if let Some(project_id) = project_id {
        repo.get_for_project(project_id, lane)
            .await
            .inspect_err(|error| {
                tracing::warn!(
                    %project_id,
                    lane = %lane,
                    %error,
                    "Failed to fetch project-scoped agent lane settings"
                );
            })
            .ok()
            .flatten()
    } else {
        None
    };

    let global_row = repo
        .get_global(lane)
        .await
        .inspect_err(|error| {
            tracing::warn!(
                lane = %lane,
                %error,
                "Failed to fetch global agent lane settings"
            );
        })
        .ok()
        .flatten();

    (project_row, global_row)
}

async fn load_legacy_ideation_lane_settings(
    lane: AgentLane,
    project_id: Option<&str>,
    ideation_effort_settings_repo: Option<&Arc<dyn IdeationEffortSettingsRepository>>,
    ideation_model_settings_repo: Option<&Arc<dyn IdeationModelSettingsRepository>>,
) -> (Option<AgentLaneSettings>, Option<AgentLaneSettings>) {
    if !matches!(
        lane,
        AgentLane::IdeationPrimary
            | AgentLane::IdeationVerifier
            | AgentLane::IdeationSubagent
            | AgentLane::IdeationVerifierSubagent
    ) {
        return (None, None);
    }

    let project_model_settings =
        load_legacy_project_model_settings(project_id, ideation_model_settings_repo).await;
    let global_model_settings =
        load_legacy_global_model_settings(ideation_model_settings_repo).await;
    let project_effort_settings =
        load_legacy_effort_settings(project_id, ideation_effort_settings_repo).await;
    let global_effort_settings =
        load_legacy_effort_settings(None, ideation_effort_settings_repo).await;

    (
        legacy_lane_settings_from_rows(
            lane,
            project_model_settings.as_ref(),
            project_effort_settings.as_ref(),
        ),
        legacy_lane_settings_from_rows(
            lane,
            global_model_settings.as_ref(),
            global_effort_settings.as_ref(),
        ),
    )
}

async fn load_legacy_project_model_settings(
    project_id: Option<&str>,
    repo: Option<&Arc<dyn IdeationModelSettingsRepository>>,
) -> Option<IdeationModelSettings> {
    let Some(project_id) = project_id else {
        return None;
    };
    let Some(repo) = repo else {
        return None;
    };

    repo.get_for_project(project_id)
        .await
        .inspect_err(|error| {
            tracing::warn!(
                %project_id,
                %error,
                "Failed to fetch legacy ideation model settings"
            );
        })
        .ok()
        .flatten()
}

async fn load_legacy_global_model_settings(
    repo: Option<&Arc<dyn IdeationModelSettingsRepository>>,
) -> Option<IdeationModelSettings> {
    let Some(repo) = repo else {
        return None;
    };

    repo.get_global()
        .await
        .inspect_err(|error| {
            tracing::warn!(%error, "Failed to fetch global legacy ideation model settings");
        })
        .ok()
        .flatten()
}

async fn load_legacy_effort_settings(
    project_id: Option<&str>,
    repo: Option<&Arc<dyn IdeationEffortSettingsRepository>>,
) -> Option<IdeationEffortSettings> {
    let Some(repo) = repo else {
        return None;
    };

    repo.get_by_project_id(project_id)
        .await
        .inspect_err(|error| {
            if let Some(project_id) = project_id {
                tracing::warn!(
                    %project_id,
                    %error,
                    "Failed to fetch legacy ideation effort settings"
                );
            } else {
                tracing::warn!(%error, "Failed to fetch global legacy ideation effort settings");
            }
        })
        .ok()
        .flatten()
}

fn legacy_lane_settings_from_rows(
    lane: AgentLane,
    model_settings: Option<&IdeationModelSettings>,
    effort_settings: Option<&IdeationEffortSettings>,
) -> Option<AgentLaneSettings> {
    let model = match lane {
        AgentLane::IdeationPrimary => {
            legacy_model_value(model_settings.map(|settings| &settings.primary_model))
        }
        AgentLane::IdeationVerifier => {
            legacy_model_value(model_settings.map(|settings| &settings.verifier_model))
        }
        AgentLane::IdeationSubagent => {
            legacy_model_value(model_settings.map(|settings| &settings.ideation_subagent_model))
        }
        AgentLane::IdeationVerifierSubagent => {
            legacy_model_value(model_settings.map(|settings| &settings.verifier_subagent_model))
        }
        AgentLane::ExecutionWorker
        | AgentLane::ExecutionReviewer
        | AgentLane::ExecutionReexecutor
        | AgentLane::ExecutionMerger
        | AgentLane::ExecutionBranchUpdater => None,
    };
    let effort = match lane {
        AgentLane::IdeationPrimary => {
            legacy_effort_value(effort_settings.map(|settings| &settings.primary_effort))
        }
        AgentLane::IdeationVerifier => {
            legacy_effort_value(effort_settings.map(|settings| &settings.verifier_effort))
        }
        AgentLane::IdeationSubagent
        | AgentLane::IdeationVerifierSubagent
        | AgentLane::ExecutionWorker
        | AgentLane::ExecutionReviewer
        | AgentLane::ExecutionReexecutor
        | AgentLane::ExecutionMerger
        | AgentLane::ExecutionBranchUpdater => None,
    };

    if model.is_none() && effort.is_none() {
        return None;
    }

    Some(AgentLaneSettings {
        harness: AgentHarnessKind::Claude,
        model,
        effort,
        approval_policy: None,
        sandbox_mode: None,
    })
}

fn legacy_model_value(level: Option<&ModelLevel>) -> Option<String> {
    match level {
        Some(ModelLevel::Sonnet) => Some("sonnet".to_string()),
        Some(ModelLevel::Opus) => Some("opus".to_string()),
        Some(ModelLevel::Haiku) => Some("haiku".to_string()),
        Some(ModelLevel::Fable) => Some("fable".to_string()),
        Some(ModelLevel::Inherit) | None => None,
    }
}

fn legacy_effort_value(level: Option<&EffortLevel>) -> Option<LogicalEffort> {
    match level {
        Some(EffortLevel::Low) => Some(LogicalEffort::Low),
        Some(EffortLevel::Medium) => Some(LogicalEffort::Medium),
        Some(EffortLevel::High) => Some(LogicalEffort::High),
        Some(EffortLevel::Max) => Some(LogicalEffort::XHigh),
        Some(EffortLevel::Inherit) | None => None,
    }
}

fn should_prefer_legacy_global_settings(
    lane: AgentLane,
    global_row: Option<&StoredAgentLaneSettings>,
    legacy_global_settings: Option<&AgentLaneSettings>,
) -> bool {
    let Some(global_row) = global_row else {
        return false;
    };
    let Some(legacy_global_settings) = legacy_global_settings else {
        return false;
    };
    let Some(default_settings) = default_agent_harness_settings_config().get(&lane) else {
        return false;
    };

    global_row.settings == *default_settings && global_row.settings != *legacy_global_settings
}

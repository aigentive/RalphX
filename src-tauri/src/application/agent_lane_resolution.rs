use std::sync::Arc;

use crate::domain::agents::{
    generic_harness_lane_defaults, AgentHarnessKind, AgentLane, AgentLaneSettings, LogicalEffort,
    StoredAgentLaneSettings, DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::ChatContextType;
use crate::domain::repositories::AgentLaneSettingsRepository;
use crate::infrastructure::agents::claude::{canonical_short_agent_name, resolve_model};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAgentSpawnSettings {
    pub configured_harness: Option<AgentHarnessKind>,
    pub effective_harness: AgentHarnessKind,
    pub configured_model: Option<String>,
    pub configured_logical_effort: Option<LogicalEffort>,
    pub configured_approval_policy: Option<String>,
    pub configured_sandbox_mode: Option<String>,
    pub model: String,
    pub logical_effort: Option<LogicalEffort>,
    pub claude_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub configured_subagent_model_cap: Option<String>,
    pub subagent_model_cap: Option<String>,
}

pub(crate) async fn resolve_agent_spawn_settings(
    agent_name: &str,
    project_id: Option<&str>,
    context_type: ChatContextType,
    entity_status: Option<&str>,
    harness_override: Option<AgentHarnessKind>,
    model_override: Option<&str>,
    agent_lane_settings_repo: Option<&Arc<dyn AgentLaneSettingsRepository>>,
) -> ResolvedAgentSpawnSettings {
    let primary_lane = lane_for_context(agent_name, context_type, entity_status);
    let subagent_lane = subagent_lane_for_context(agent_name, context_type);

    if primary_lane.is_none() {
        let effective_harness = harness_override.unwrap_or(DEFAULT_AGENT_HARNESS);
        let non_lane_defaults = non_lane_harness_defaults(effective_harness);
        return ResolvedAgentSpawnSettings {
            configured_harness: None,
            effective_harness,
            configured_model: None,
            configured_logical_effort: None,
            configured_approval_policy: None,
            configured_sandbox_mode: None,
            model: model_override
                .map(str::to_string)
                .or_else(|| {
                    non_lane_defaults
                        .as_ref()
                        .and_then(|settings| settings.model.clone())
                })
                .unwrap_or_else(|| resolve_model(Some(agent_name))),
            logical_effort: None,
            claude_effort: None,
            approval_policy: non_lane_defaults
                .as_ref()
                .and_then(|settings| settings.approval_policy.clone()),
            sandbox_mode: non_lane_defaults
                .as_ref()
                .and_then(|settings| settings.sandbox_mode.clone()),
            configured_subagent_model_cap: None,
            subagent_model_cap: None,
        };
    }

    let (primary_project_row, primary_global_row) =
        load_lane_rows(agent_lane_settings_repo, project_id, primary_lane).await;
    let configured_harness =
        lane_harness(primary_project_row.as_ref(), primary_global_row.as_ref());
    let effective_harness = harness_override
        .or(configured_harness)
        .unwrap_or(DEFAULT_AGENT_HARNESS);
    let settings_match_effective_harness = configured_harness
        .map(|configured| configured == effective_harness)
        .unwrap_or(true);
    let configured_primary_settings = settings_match_effective_harness
        .then(|| lane_settings_value(primary_project_row.as_ref(), primary_global_row.as_ref()))
        .flatten();
    let configured_harness =
        configured_harness.filter(|configured| *configured == effective_harness);
    let harness_primary_defaults =
        primary_lane.and_then(|lane| nondefault_harness_lane_settings(lane, effective_harness));

    let model = if let Some(model_override) = model_override {
        model_override.to_string()
    } else if let Some(model) = configured_primary_settings
        .as_ref()
        .and_then(|settings| settings.model.clone())
    {
        model
    } else if let Some(model) = harness_primary_defaults
        .as_ref()
        .and_then(|settings| settings.model.clone())
    {
        model
    } else {
        resolve_model(Some(agent_name))
    };

    let logical_effort = if primary_lane.is_some() {
        if let Some(effort) = configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.effort)
        {
            Some(effort)
        } else if let Some(defaults) = harness_primary_defaults.as_ref() {
            defaults.effort
        } else {
            None
        }
    } else {
        None
    };

    let (configured_subagent_model_cap, subagent_model_cap) = if let Some(subagent_lane) =
        subagent_lane
    {
        let (subagent_project_row, subagent_global_row) =
            load_lane_rows(agent_lane_settings_repo, project_id, Some(subagent_lane)).await;
        let subagent_harness =
            lane_harness(subagent_project_row.as_ref(), subagent_global_row.as_ref());
        let configured_subagent_model_cap = subagent_harness
            .map(|configured| configured == effective_harness)
            .unwrap_or(true)
            .then(|| {
                lane_settings_value(subagent_project_row.as_ref(), subagent_global_row.as_ref())
                    .and_then(|settings| settings.model)
            })
            .flatten();

        let subagent_model_cap = if let Some(model) = configured_subagent_model_cap.clone() {
            model
        } else if let Some(model) =
            nondefault_harness_lane_settings(subagent_lane, effective_harness)
                .and_then(|settings| settings.model)
        {
            model
        } else {
            "haiku".to_string()
        };

        (configured_subagent_model_cap, Some(subagent_model_cap))
    } else {
        (None, None)
    };

    ResolvedAgentSpawnSettings {
        configured_harness,
        effective_harness,
        configured_model: configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.model.clone()),
        configured_logical_effort: configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.effort),
        configured_approval_policy: configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.approval_policy.clone()),
        configured_sandbox_mode: configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.sandbox_mode.clone()),
        model,
        logical_effort,
        claude_effort: logical_effort.map(|effort| effort.to_legacy_claude_effort().to_string()),
        approval_policy: default_approval_policy_for_harness(effective_harness)
            .map(str::to_string)
            .or_else(|| {
                configured_primary_settings
                    .as_ref()
                    .and_then(|settings| settings.approval_policy.clone())
            })
            .or_else(|| {
                harness_primary_defaults
                    .as_ref()
                    .and_then(|settings| settings.approval_policy.clone())
            }),
        sandbox_mode: default_sandbox_mode_for_harness(effective_harness)
            .map(str::to_string)
            .or_else(|| {
                configured_primary_settings
                    .as_ref()
                    .and_then(|settings| settings.sandbox_mode.clone())
            })
            .or_else(|| {
                harness_primary_defaults
                    .as_ref()
                    .and_then(|settings| settings.sandbox_mode.clone())
            }),
        configured_subagent_model_cap,
        subagent_model_cap,
    }
}

pub(crate) async fn resolve_agent_subagent_harness(
    agent_name: &str,
    project_id: Option<&str>,
    context_type: ChatContextType,
    entity_status: Option<&str>,
    agent_lane_settings_repo: Option<&Arc<dyn AgentLaneSettingsRepository>>,
) -> AgentHarnessKind {
    let primary = resolve_agent_spawn_settings(
        agent_name,
        project_id,
        context_type,
        entity_status,
        None,
        None,
        agent_lane_settings_repo,
    )
    .await;

    let Some(subagent_lane) = subagent_lane_for_context(agent_name, context_type) else {
        return primary.effective_harness;
    };

    let (subagent_project_row, subagent_global_row) =
        load_lane_rows(agent_lane_settings_repo, project_id, Some(subagent_lane)).await;

    lane_harness(subagent_project_row.as_ref(), subagent_global_row.as_ref())
        .unwrap_or(primary.effective_harness)
}

fn ideation_lane_for_agent(agent_name: &str) -> Option<AgentLane> {
    let normalized = canonical_short_agent_name(agent_name);
    match normalized {
        "ralphx-ideation"
        | "ralphx-ideation-team-lead"
        | "ideation-team-member"
        | "ralphx-ideation-readonly" => Some(AgentLane::IdeationPrimary),
        "ralphx-plan-verifier" => Some(AgentLane::IdeationVerifier),
        _ => None,
    }
}

fn execution_lane_for_context(
    context_type: ChatContextType,
    entity_status: Option<&str>,
) -> Option<AgentLane> {
    match context_type {
        ChatContextType::TaskExecution => {
            if matches!(entity_status, Some("re_executing")) {
                Some(AgentLane::ExecutionReexecutor)
            } else {
                Some(AgentLane::ExecutionWorker)
            }
        }
        ChatContextType::Review => Some(AgentLane::ExecutionReviewer),
        ChatContextType::Merge => Some(AgentLane::ExecutionMerger),
        ChatContextType::Ideation
        | ChatContextType::Delegation
        | ChatContextType::Task
        | ChatContextType::Project => None,
    }
}

fn lane_for_context(
    agent_name: &str,
    context_type: ChatContextType,
    entity_status: Option<&str>,
) -> Option<AgentLane> {
    match context_type {
        ChatContextType::Ideation => ideation_lane_for_agent(agent_name),
        _ => execution_lane_for_context(context_type, entity_status),
    }
}

fn ideation_subagent_lane_for_agent(agent_name: &str) -> Option<AgentLane> {
    ideation_lane_for_agent(agent_name).map(|lane| match lane {
        AgentLane::IdeationVerifier => AgentLane::IdeationVerifierSubagent,
        AgentLane::IdeationPrimary => AgentLane::IdeationSubagent,
        _ => unreachable!("ideation lane mapper returned a non-ideation lane"),
    })
}

fn subagent_lane_for_context(agent_name: &str, context_type: ChatContextType) -> Option<AgentLane> {
    match context_type {
        ChatContextType::Ideation => ideation_subagent_lane_for_agent(agent_name),
        _ => None,
    }
}

async fn load_lane_rows(
    repo: Option<&Arc<dyn AgentLaneSettingsRepository>>,
    project_id: Option<&str>,
    lane: Option<AgentLane>,
) -> (
    Option<StoredAgentLaneSettings>,
    Option<StoredAgentLaneSettings>,
) {
    let Some(repo) = repo else {
        return (None, None);
    };
    let Some(lane) = lane else {
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

fn lane_settings_value(
    project_row: Option<&StoredAgentLaneSettings>,
    global_row: Option<&StoredAgentLaneSettings>,
) -> Option<AgentLaneSettings> {
    project_row
        .map(|row| row.settings.clone())
        .or_else(|| global_row.map(|row| row.settings.clone()))
}

fn lane_harness(
    project_row: Option<&StoredAgentLaneSettings>,
    global_row: Option<&StoredAgentLaneSettings>,
) -> Option<AgentHarnessKind> {
    project_row
        .map(|row| row.settings.harness)
        .or_else(|| global_row.map(|row| row.settings.harness))
}

fn nondefault_harness_lane_settings(
    lane: AgentLane,
    harness: AgentHarnessKind,
) -> Option<AgentLaneSettings> {
    if harness == DEFAULT_AGENT_HARNESS {
        return None;
    }

    Some(generic_harness_lane_defaults(harness, lane))
}

fn non_lane_harness_defaults(harness: AgentHarnessKind) -> Option<AgentLaneSettings> {
    nondefault_harness_lane_settings(AgentLane::IdeationPrimary, harness)
}

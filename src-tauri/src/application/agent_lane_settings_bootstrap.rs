use crate::domain::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings};
use crate::domain::repositories::AgentLaneSettingsRepository;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaneSettingsBootstrapResult {
    pub global_defaults: HashMap<AgentLane, AgentLaneSettings>,
    pub seeded_global_lanes: Vec<AgentLane>,
    pub upgraded_global_lanes: Vec<AgentLane>,
}

pub async fn load_or_seed_agent_lane_settings_defaults(
    agent_lane_settings_repo: Arc<dyn AgentLaneSettingsRepository>,
    desired_global_defaults: &HashMap<AgentLane, AgentLaneSettings>,
) -> Result<AgentLaneSettingsBootstrapResult, String> {
    let existing_rows = agent_lane_settings_repo
        .list_global()
        .await
        .map_err(|error| error.to_string())?;
    let mut resolved_defaults = existing_rows
        .into_iter()
        .map(|row| (row.lane, row.settings))
        .collect::<HashMap<_, _>>();
    let mut seeded_global_lanes = Vec::new();
    let mut upgraded_global_lanes = Vec::new();

    for (lane, settings) in desired_global_defaults {
        if let Some(existing) = resolved_defaults.get(lane).cloned() {
            if should_upgrade_legacy_codex_lane(*lane, &existing, settings) {
                let row = agent_lane_settings_repo
                    .upsert_global(*lane, settings)
                    .await
                    .map_err(|error| error.to_string())?;
                resolved_defaults.insert(row.lane, row.settings);
                upgraded_global_lanes.push(*lane);
            }
            continue;
        }

        let row = agent_lane_settings_repo
            .upsert_global(*lane, settings)
            .await
            .map_err(|error| error.to_string())?;
        resolved_defaults.insert(row.lane, row.settings);
        seeded_global_lanes.push(*lane);
    }

    seeded_global_lanes.sort_by_key(|lane| lane.to_string());
    upgraded_global_lanes.sort_by_key(|lane| lane.to_string());

    Ok(AgentLaneSettingsBootstrapResult {
        global_defaults: resolved_defaults,
        seeded_global_lanes,
        upgraded_global_lanes,
    })
}

fn should_upgrade_legacy_codex_lane(
    lane: AgentLane,
    existing: &AgentLaneSettings,
    desired: &AgentLaneSettings,
) -> bool {
    if desired.harness != AgentHarnessKind::Codex || existing.harness != AgentHarnessKind::Codex {
        return false;
    }

    if existing == desired {
        return false;
    }

    if existing.model != desired.model || existing.effort != desired.effort {
        return false;
    }

    if desired.approval_policy.as_deref() != Some("never")
        || desired.sandbox_mode.as_deref() != Some("danger-full-access")
    {
        return false;
    }

    let legacy_pairs: &[(Option<&str>, Option<&str>)] = match lane {
        AgentLane::IdeationPrimary
        | AgentLane::IdeationVerifier
        | AgentLane::ExecutionWorker
        | AgentLane::ExecutionReviewer
        | AgentLane::ExecutionReexecutor
        | AgentLane::ExecutionMerger => &[
            (Some("on-request"), Some("workspace-write")),
            (Some("never"), Some("workspace-write")),
        ],
        AgentLane::IdeationSubagent | AgentLane::IdeationVerifierSubagent => &[
            (None, None),
            (Some("never"), Some("workspace-write")),
        ],
    };

    legacy_pairs.iter().any(|(approval, sandbox)| {
        existing.approval_policy.as_deref() == *approval
            && existing.sandbox_mode.as_deref() == *sandbox
    })
}

#[cfg(test)]
#[path = "agent_lane_settings_bootstrap_tests.rs"]
mod tests;

// Ideation effort resolution for Claude agent spawns.
//
// Resolves the `--effort` level for ideation agents using a 4-level priority chain:
// per-project DB row → global DB row → YAML agent config → YAML default.

use crate::domain::ideation::{EffortBucket, EffortLevel};
use crate::domain::repositories::IdeationEffortSettingsRepository;

use super::{canonical_short_agent_name, resolve_effort};

/// Map an agent name to its ideation effort bucket.
///
/// Returns `Some(bucket)` for the known ideation agents, or `None` for all others
/// (non-ideation agents fall through to the standard YAML-based resolution).
pub fn effort_bucket_for_agent(agent_name: &str) -> Option<EffortBucket> {
    let normalized = canonical_short_agent_name(agent_name);
    match normalized {
        "ralphx-ideation" | "ralphx-ideation-readonly" => Some(EffortBucket::Primary),
        _ => None,
    }
}

/// Resolve the `--effort` value for an ideation agent using a 4-level chain:
///
/// 1. Per-project DB row for `project_id` (if `Some`) — bucket effort if not `Inherit`
/// 2. Global DB row (`project_id = NULL`) — bucket effort if not `Inherit`
/// 3. YAML agent-level config (`AgentConfig.effort`)
/// 4. YAML `default_effort` from `ClaudeRuntimeConfig`
///
/// If the agent is not an ideation agent (bucket = `None`), falls through directly
/// to `resolve_effort(Some(agent_name))` (levels 3–4).
pub async fn resolve_ideation_effort(
    agent_name: &str,
    project_id: Option<&str>,
    repo: &dyn IdeationEffortSettingsRepository,
) -> String {
    let bucket = match effort_bucket_for_agent(agent_name) {
        Some(b) => b,
        None => return resolve_effort(Some(agent_name)),
    };

    // Level 1: per-project override
    if let Some(pid) = project_id {
        if let Ok(Some(settings)) = repo.get_by_project_id(Some(pid)).await {
            let level = settings.effort_for_bucket(&bucket);
            if *level != EffortLevel::Inherit {
                return level.to_string();
            }
        }
    }

    // Level 2: global row
    if let Ok(Some(settings)) = repo.get_by_project_id(None).await {
        let level = settings.effort_for_bucket(&bucket);
        if *level != EffortLevel::Inherit {
            return level.to_string();
        }
    }

    // Levels 3–4: YAML agent config + YAML default
    resolve_effort_with_source(Some(agent_name)).0
}

/// Resolve effort from YAML config and return `(effort, source)`.
///
/// Returns `(yaml_effort, "yaml")` if an explicit YAML effort is configured for the agent,
/// or `(default_effort, "default")` as the Claude runtime fallback.
pub fn resolve_effort_with_source(agent_type: Option<&str>) -> (String, String) {
    use super::{claude_runtime_config, get_agent_config};

    let yaml_effort = agent_type
        .and_then(|name| get_agent_config(name))
        .and_then(|config| config.effort.clone());

    if let Some(effort) = yaml_effort {
        return (effort, "yaml".to_string());
    }

    (
        claude_runtime_config().default_effort.clone(),
        "default".to_string(),
    )
}

#[cfg(test)]
#[path = "effort_resolver_tests.rs"]
mod tests;

use super::*;
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings, LogicalEffort};
use std::collections::HashMap;
use std::sync::Arc;

fn codex_defaults(model: &str, effort: LogicalEffort) -> AgentLaneSettings {
    let mut settings = AgentLaneSettings::new(AgentHarnessKind::Codex);
    settings.model = Some(model.to_string());
    settings.effort = Some(effort);
    settings.approval_policy = Some("never".to_string());
    settings.sandbox_mode = Some("danger-full-access".to_string());
    settings
}

#[tokio::test]
async fn test_load_or_seed_agent_lane_settings_defaults_seeds_missing_global_rows() {
    let app_state = AppState::new_test();
    let desired_defaults = HashMap::from([
        (
            AgentLane::IdeationPrimary,
            codex_defaults("gpt-5.4", LogicalEffort::XHigh),
        ),
        (
            AgentLane::IdeationVerifier,
            codex_defaults("gpt-5.4-mini", LogicalEffort::Medium),
        ),
        (
            AgentLane::ExecutionWorker,
            AgentLaneSettings::new(AgentHarnessKind::Claude),
        ),
        (
            AgentLane::ExecutionReviewer,
            AgentLaneSettings::new(AgentHarnessKind::Claude),
        ),
    ]);

    let result = load_or_seed_agent_lane_settings_defaults(
        Arc::clone(&app_state.agent_lane_settings_repo),
        &desired_defaults,
    )
    .await
    .unwrap();

    assert_eq!(
        result.seeded_global_lanes,
        vec![
            AgentLane::ExecutionReviewer,
            AgentLane::ExecutionWorker,
            AgentLane::IdeationPrimary,
            AgentLane::IdeationVerifier,
        ]
    );
    assert_eq!(
        result.global_defaults.get(&AgentLane::IdeationPrimary),
        desired_defaults.get(&AgentLane::IdeationPrimary)
    );
    assert_eq!(
        result.global_defaults.get(&AgentLane::IdeationVerifier),
        desired_defaults.get(&AgentLane::IdeationVerifier)
    );
    assert_eq!(
        result.global_defaults.get(&AgentLane::ExecutionWorker),
        desired_defaults.get(&AgentLane::ExecutionWorker)
    );
    assert_eq!(
        result.global_defaults.get(&AgentLane::ExecutionReviewer),
        desired_defaults.get(&AgentLane::ExecutionReviewer)
    );
    assert!(result.upgraded_global_lanes.is_empty());
}

#[tokio::test]
async fn test_load_or_seed_agent_lane_settings_defaults_preserves_existing_rows() {
    let app_state = AppState::new_test();
    let stored_defaults = AgentLaneSettings::new(AgentHarnessKind::Claude);
    let desired_defaults = HashMap::from([
        (
            AgentLane::IdeationPrimary,
            codex_defaults("gpt-5.4", LogicalEffort::XHigh),
        ),
        (
            AgentLane::IdeationVerifier,
            codex_defaults("gpt-5.4-mini", LogicalEffort::Medium),
        ),
        (
            AgentLane::ExecutionWorker,
            AgentLaneSettings::new(AgentHarnessKind::Claude),
        ),
    ]);

    app_state
        .agent_lane_settings_repo
        .upsert_global(AgentLane::IdeationPrimary, &stored_defaults)
        .await
        .unwrap();

    let result = load_or_seed_agent_lane_settings_defaults(
        Arc::clone(&app_state.agent_lane_settings_repo),
        &desired_defaults,
    )
    .await
    .unwrap();

    assert_eq!(
        result.seeded_global_lanes,
        vec![AgentLane::ExecutionWorker, AgentLane::IdeationVerifier]
    );
    assert_eq!(
        result.global_defaults.get(&AgentLane::IdeationPrimary),
        Some(&stored_defaults)
    );
    assert_eq!(
        result.global_defaults.get(&AgentLane::IdeationVerifier),
        desired_defaults.get(&AgentLane::IdeationVerifier)
    );
    assert_eq!(
        result.global_defaults.get(&AgentLane::ExecutionWorker),
        desired_defaults.get(&AgentLane::ExecutionWorker)
    );
    assert!(result.upgraded_global_lanes.is_empty());
}

#[tokio::test]
async fn test_load_or_seed_agent_lane_settings_defaults_leaves_empty_desired_defaults_alone() {
    let app_state = AppState::new_test();
    let desired_defaults = HashMap::new();

    let result = load_or_seed_agent_lane_settings_defaults(
        Arc::clone(&app_state.agent_lane_settings_repo),
        &desired_defaults,
    )
    .await
    .unwrap();

    assert!(result.seeded_global_lanes.is_empty());
    assert!(result.global_defaults.is_empty());
    assert!(result.upgraded_global_lanes.is_empty());
}

#[tokio::test]
async fn test_load_or_seed_agent_lane_settings_defaults_upgrades_legacy_codex_global_rows() {
    let app_state = AppState::new_test();
    let desired_defaults = HashMap::from([
        (
            AgentLane::IdeationPrimary,
            codex_defaults("gpt-5.4", LogicalEffort::XHigh),
        ),
        (
            AgentLane::IdeationSubagent,
            codex_defaults("gpt-5.4-mini", LogicalEffort::Medium),
        ),
    ]);

    let mut legacy_primary = codex_defaults("gpt-5.4", LogicalEffort::XHigh);
    legacy_primary.approval_policy = Some("on-request".to_string());
    legacy_primary.sandbox_mode = Some("workspace-write".to_string());
    let mut legacy_subagent = codex_defaults("gpt-5.4-mini", LogicalEffort::Medium);
    legacy_subagent.approval_policy = None;
    legacy_subagent.sandbox_mode = None;

    app_state
        .agent_lane_settings_repo
        .upsert_global(AgentLane::IdeationPrimary, &legacy_primary)
        .await
        .unwrap();
    app_state
        .agent_lane_settings_repo
        .upsert_global(AgentLane::IdeationSubagent, &legacy_subagent)
        .await
        .unwrap();

    let result = load_or_seed_agent_lane_settings_defaults(
        Arc::clone(&app_state.agent_lane_settings_repo),
        &desired_defaults,
    )
    .await
    .unwrap();

    assert!(result.seeded_global_lanes.is_empty());
    assert_eq!(
        result.upgraded_global_lanes,
        vec![AgentLane::IdeationPrimary, AgentLane::IdeationSubagent]
    );
    assert_eq!(
        result.global_defaults.get(&AgentLane::IdeationPrimary),
        desired_defaults.get(&AgentLane::IdeationPrimary)
    );
    assert_eq!(
        result.global_defaults.get(&AgentLane::IdeationSubagent),
        desired_defaults.get(&AgentLane::IdeationSubagent)
    );
}

#[tokio::test]
async fn test_load_or_seed_agent_lane_settings_defaults_upgrades_stale_never_workspace_write_codex_rows()
{
    let app_state = AppState::new_test();
    let desired_defaults = HashMap::from([(
        AgentLane::ExecutionWorker,
        codex_defaults("gpt-5.4", LogicalEffort::XHigh),
    )]);

    let mut stale = codex_defaults("gpt-5.4", LogicalEffort::XHigh);
    stale.sandbox_mode = Some("workspace-write".to_string());

    app_state
        .agent_lane_settings_repo
        .upsert_global(AgentLane::ExecutionWorker, &stale)
        .await
        .unwrap();

    let result = load_or_seed_agent_lane_settings_defaults(
        Arc::clone(&app_state.agent_lane_settings_repo),
        &desired_defaults,
    )
    .await
    .unwrap();

    assert!(result.seeded_global_lanes.is_empty());
    assert_eq!(result.upgraded_global_lanes, vec![AgentLane::ExecutionWorker]);
    assert_eq!(
        result.global_defaults.get(&AgentLane::ExecutionWorker),
        desired_defaults.get(&AgentLane::ExecutionWorker)
    );
}

#[tokio::test]
async fn test_load_or_seed_agent_lane_settings_defaults_preserves_custom_codex_rows() {
    let app_state = AppState::new_test();
    let desired_defaults = HashMap::from([(
        AgentLane::IdeationPrimary,
        codex_defaults("gpt-5.4", LogicalEffort::XHigh),
    )]);

    let mut custom = codex_defaults("gpt-5.4", LogicalEffort::XHigh);
    custom.sandbox_mode = Some("read-only".to_string());

    app_state
        .agent_lane_settings_repo
        .upsert_global(AgentLane::IdeationPrimary, &custom)
        .await
        .unwrap();

    let result = load_or_seed_agent_lane_settings_defaults(
        Arc::clone(&app_state.agent_lane_settings_repo),
        &desired_defaults,
    )
    .await
    .unwrap();

    assert!(result.seeded_global_lanes.is_empty());
    assert!(result.upgraded_global_lanes.is_empty());
    assert_eq!(
        result.global_defaults.get(&AgentLane::IdeationPrimary),
        Some(&custom)
    );
}

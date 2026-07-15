use super::*;

use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentLaneSettings, AgentProviderCliManagementMode, AgentProviderSettings,
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

fn write_executable(path: &std::path::Path, contents: &str) {
    std::fs::write(path, contents).expect("write fake codex");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("fake codex metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod fake codex");
    }
}

fn write_modern_codex_cli(path: &std::path::Path) {
    write_executable(
        path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.144.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --search' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
elif [ "$1" = "features" ] && [ "$2" = "list" ]; then
  printf '%s\n' 'fast_mode stable true'
elif [ "$1" = "debug" ] && [ "$2" = "models" ] && [ -z "$3" ]; then
  printf '%s\n' '{"models":[{"slug":"gpt-5.5","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"}],"additional_speed_tiers":["fast"]}]}'
elif [ "$1" = "debug" ] && [ "$2" = "models" ] && [ "$3" = "--bundled" ]; then
  printf '%s\n' '{"models":[{"slug":"gpt-5.5","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"}],"additional_speed_tiers":["fast"]}]}'
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );
}

#[tokio::test]
async fn availability_helper_loads_stored_provider_probes_for_requested_lanes() {
    let state = AppState::new_test();
    let project_id = Some("project-availability".to_string());

    let responses =
        get_harness_availability_for_lanes(project_id.clone(), &state, &IDEATION_LANES, false)
            .await
            .expect("availability should load");

    assert_eq!(responses.len(), IDEATION_LANES.len());
    assert!(responses
        .iter()
        .all(|response| response.project_id == project_id));
    assert!(responses
        .iter()
        .all(|response| !response.effective_harness.is_empty()));
}

#[test]
fn availability_response_maps_lane_probe_and_error_fields() {
    let availability = crate::application::ideation_harness_availability::LaneHarnessAvailability {
        lane: AgentLane::ExecutionWorker,
        configured_harness: Some(AgentHarnessKind::Codex),
        effective_harness: AgentHarnessKind::Codex,
        binary_path: Some("/usr/local/bin/codex".to_string()),
        binary_found: true,
        probe_succeeded: false,
        available: false,
        missing_core_exec_features: vec!["exec".to_string()],
        error: Some("codex missing exec support".to_string()),
    };

    let response = to_response(&Some("project-1".to_string()), availability);

    assert_eq!(response.project_id.as_deref(), Some("project-1"));
    assert_eq!(response.lane, "execution_worker");
    assert_eq!(response.configured_harness.as_deref(), Some("codex"));
    assert_eq!(response.effective_harness, "codex");
    assert_eq!(
        response.binary_path.as_deref(),
        Some("/usr/local/bin/codex")
    );
    assert!(response.binary_found);
    assert!(!response.probe_succeeded);
    assert!(!response.available);
    assert_eq!(
        response.missing_core_exec_features,
        vec!["exec".to_string()]
    );
    assert_eq!(
        response.error.as_deref(),
        Some("codex missing exec support")
    );
}

#[tokio::test]
async fn ideation_availability_command_uses_stored_provider_snapshot() {
    let state = AppState::new_test();
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let responses = get_ideation_harness_availability(
        Some(AgentHarnessAvailabilityInput {
            project_id: Some("project-command".to_string()),
            refresh_runtime: false,
        }),
        app.state(),
    )
    .await
    .expect("ideation availability should load");

    assert_eq!(responses.len(), IDEATION_LANES.len());
    assert!(responses
        .iter()
        .all(|response| response.project_id.as_deref() == Some("project-command")));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn refreshed_ideation_availability_overlays_rx_managed_codex_probe() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let managed_codex_path = temp_dir.path().join("codex");
    write_modern_codex_cli(&managed_codex_path);
    let _override =
        crate::application::managed_provider_cli::override_managed_codex_binary_path_for_tests(
            managed_codex_path.clone(),
        );
    let state = AppState::new_test();
    let mut provider = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    provider.enabled = true;
    provider.is_default = true;
    provider.cli_management_mode = AgentProviderCliManagementMode::RxManaged;
    state
        .agent_provider_settings_repo
        .upsert(&provider)
        .await
        .expect("upsert Codex provider settings");
    state
        .agent_lane_settings_repo
        .upsert_global(
            AgentLane::IdeationPrimary,
            &AgentLaneSettings::new(AgentHarnessKind::Codex),
        )
        .await
        .expect("upsert Codex ideation lane");

    let responses = get_harness_availability_for_lanes(None, &state, &IDEATION_LANES, true)
        .await
        .expect("availability should load");

    let primary = responses
        .iter()
        .find(|response| response.lane == AgentLane::IdeationPrimary.to_string())
        .expect("primary ideation response");
    assert_eq!(primary.effective_harness, "codex");
    assert!(primary.available);
    assert_eq!(
        primary.binary_path.as_deref(),
        Some(managed_codex_path.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn agent_availability_command_defaults_input() {
    let state = AppState::new_test();
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let responses = get_agent_harness_availability(None, app.state())
        .await
        .expect("agent availability should load");

    assert_eq!(responses.len(), AGENT_LANES.len());
    assert!(responses
        .iter()
        .all(|response| response.project_id.is_none()));
}

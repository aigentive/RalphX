use crate::application::ideation_harness_availability::validate_claude_runtime_path;
use crate::application::ideation_harness_availability::LaneHarnessAvailability;
use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, AgentProviderCliManagementMode,
    AgentProviderSettings,
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

fn availability(
    effective_harness: AgentHarnessKind,
    available: bool,
    error: Option<&str>,
) -> LaneHarnessAvailability {
    LaneHarnessAvailability {
        lane: AgentLane::IdeationPrimary,
        configured_harness: Some(effective_harness),
        effective_harness,
        binary_path: None,
        binary_found: available,
        probe_succeeded: available,
        available,
        missing_core_exec_features: Vec::new(),
        error: error.map(str::to_string),
    }
}

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
#[allow(clippy::await_holding_lock)]
async fn orchestrator_availability_uses_provider_aware_primary_lane() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let managed_codex_path = temp_dir.path().join("codex");
    write_modern_codex_cli(&managed_codex_path);
    let _override =
        crate::application::managed_provider_cli::override_managed_codex_binary_path_for_tests(
            managed_codex_path,
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
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let available = super::is_orchestrator_available(app.state())
        .await
        .expect("orchestrator availability should resolve");

    assert!(available);
}

#[test]
fn deprecated_orchestrator_path_accepts_claude() {
    let result = validate_claude_runtime_path(
        &availability(AgentHarnessKind::Claude, true, None),
        "the deprecated orchestrator path",
    );

    assert!(result.is_ok());
}

#[test]
fn deprecated_orchestrator_path_rejects_unavailable_harnesses() {
    let result = validate_claude_runtime_path(
        &availability(
            AgentHarnessKind::Claude,
            false,
            Some("Claude CLI not found"),
        ),
        "the deprecated orchestrator path",
    );

    assert_eq!(result.unwrap_err(), "Claude CLI not found");
}

#[test]
fn deprecated_orchestrator_path_rejects_effective_codex() {
    let result = validate_claude_runtime_path(
        &availability(AgentHarnessKind::Codex, true, None),
        "the deprecated orchestrator path",
    );

    assert!(result
        .unwrap_err()
        .contains("deprecated orchestrator path still routes through the Claude runtime"));
}

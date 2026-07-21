use crate::application::harness_runtime_registry::HarnessRuntimeProbe;
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::utils::runtime_log_paths::{
    app_runtime_dir, managed_codex_binary_path, managed_provider_cli_dir,
};

use super::{
    checked_managed_provider_cli_launch_path, checked_provider_cli_launch_path,
    managed_probe_error, managed_provider_cli_launch_path,
    override_managed_codex_binary_path_for_tests, provider_cli_launch_path, provider_runtime_probe,
};

fn provider_settings(
    provider: AgentHarnessKind,
    mode: AgentProviderCliManagementMode,
) -> AgentProviderSettings {
    let mut settings = AgentProviderSettings::disabled_defaults(provider);
    settings.cli_management_mode = mode;
    settings
}

#[test]
fn user_managed_provider_has_no_managed_launch_override() {
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::UserManaged,
    );

    assert!(managed_provider_cli_launch_path(&settings).is_none());
    assert!(provider_runtime_probe(&settings).is_none());
}

#[test]
fn rx_managed_codex_launches_from_app_owned_binary_path() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::RxManaged,
    );

    let path = managed_provider_cli_launch_path(&settings).expect("managed Codex path");

    assert_eq!(path, managed_codex_binary_path());
    assert!(path.starts_with(managed_provider_cli_dir()));
    if cfg!(debug_assertions) {
        assert!(!path.starts_with(app_runtime_dir()));
    }
}

#[test]
fn rx_managed_native_claude_uses_default_launch_resolution() {
    let settings = provider_settings(
        AgentHarnessKind::Claude,
        AgentProviderCliManagementMode::RxManaged,
    );

    assert!(managed_provider_cli_launch_path(&settings).is_none());
    assert!(provider_runtime_probe(&settings).is_none());
}

#[test]
fn custom_codex_wrapper_path_takes_launch_precedence() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex-wrapper");
    write_modern_codex_cli(&codex_path);
    let mut settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::RxManaged,
    );
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(codex_path.to_string_lossy().into_owned());

    let launch_path = provider_cli_launch_path(&settings)
        .expect("custom launch path")
        .expect("valid custom launch path");
    let checked_path = checked_provider_cli_launch_path(&settings, "test runtime")
        .expect("checked custom launch path")
        .expect("available custom launch path");
    let probe = provider_runtime_probe(&settings).expect("custom Codex probe");

    assert_eq!(launch_path, codex_path);
    assert_eq!(checked_path, codex_path);
    assert!(probe.available);
    assert_eq!(probe.cli_version.as_deref(), Some("0.144.0"));
    assert!(probe.missing_core_exec_features.is_empty());
    assert_eq!(
        probe.supported_model_aliases,
        Some(vec![
            "gpt-5.5".to_string(),
            "gpt-5.6-sol".to_string(),
            "gpt-5.6-terra".to_string(),
        ])
    );
    assert_eq!(
        probe.supported_efforts,
        Some(vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ])
    );
}

#[test]
fn custom_binary_requires_path_before_launch() {
    let mut settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::UserManaged,
    );
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some("   ".to_string());

    let result = checked_provider_cli_launch_path(&settings, "test runtime")
        .expect("custom launch path result");
    let probe = provider_runtime_probe(&settings).expect("custom Codex probe");

    assert!(result
        .expect_err("missing custom path should fail")
        .contains("path is required"));
    assert!(!probe.available);
    assert!(!probe.binary_found);
    assert_eq!(probe.binary_path.as_deref(), Some("   "));
    assert!(probe
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("path is required"));
}

#[test]
fn custom_binary_rejects_relative_path_before_launch() {
    let mut settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::UserManaged,
    );
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some("relative/codex".to_string());

    let result = checked_provider_cli_launch_path(&settings, "test runtime")
        .expect("custom launch path result");

    assert!(result
        .expect_err("relative custom path should fail")
        .contains("absolute path"));
}

#[test]
fn custom_binary_rejects_non_launchable_file() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex-wrapper");
    std::fs::write(&codex_path, "#!/bin/sh\n").expect("write non-executable");
    let mut settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::UserManaged,
    );
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(codex_path.to_string_lossy().into_owned());

    let probe = provider_runtime_probe(&settings).expect("custom Codex probe");

    assert!(!probe.available);
    assert!(!probe.binary_found);
    assert!(probe
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("not a launchable executable file"));
}

#[test]
fn custom_codex_binary_probe_reports_probe_error() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex-wrapper");
    write_executable(
        &codex_path,
        "#!/bin/sh\nprintf 'probe failed\\n' >&2\nexit 7\n",
    );
    let mut settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::UserManaged,
    );
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(codex_path.to_string_lossy().into_owned());

    let probe = provider_runtime_probe(&settings).expect("custom Codex probe");

    assert_eq!(
        probe.binary_path.as_deref(),
        Some(codex_path.to_string_lossy().as_ref())
    );
    assert!(probe.binary_found);
    assert!(!probe.probe_succeeded);
    assert!(!probe.available);
    assert!(probe
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("probe failed"));
}

#[test]
fn custom_codex_binary_probe_reports_missing_core_features() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex-wrapper");
    write_executable(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.100.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Usage' 'Options:' '  --version'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Usage' 'Options:' '  --version'
  exit 2
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );
    let mut settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::UserManaged,
    );
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(codex_path.to_string_lossy().into_owned());

    let probe = provider_runtime_probe(&settings).expect("custom Codex probe");

    assert!(probe.binary_found);
    assert!(probe.probe_succeeded);
    assert!(!probe.available);
    assert_eq!(probe.cli_version.as_deref(), Some("0.100.0"));
    assert!(probe
        .missing_core_exec_features
        .contains(&"exec_subcommand".to_string()));
    assert!(probe
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("Custom Codex binary is missing required capability"));
}

#[test]
fn custom_claude_binary_probe_uses_selected_path() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let claude_path = temp_dir.path().join("claude-wrapper");
    write_executable(
        &claude_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'claude-code 2.1.170\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Claude Code' 'Options:' '  --model <MODEL>' '  --effort <EFFORT>'
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );
    let mut settings = provider_settings(
        AgentHarnessKind::Claude,
        AgentProviderCliManagementMode::UserManaged,
    );
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(claude_path.to_string_lossy().into_owned());

    let probe = provider_runtime_probe(&settings).expect("custom Claude probe");

    assert!(probe.available);
    assert_eq!(
        probe.binary_path.as_deref(),
        Some(claude_path.to_string_lossy().as_ref())
    );
    assert_eq!(probe.cli_version.as_deref(), Some("2.1.170"));
    assert!(probe
        .supported_model_aliases
        .as_ref()
        .is_some_and(|aliases| aliases.contains(&"fable".to_string())));
}

#[test]
fn custom_claude_binary_probe_reports_probe_error() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let claude_path = temp_dir.path().join("claude-wrapper");
    write_executable(
        &claude_path,
        "#!/bin/sh\nprintf 'probe failed\\n' >&2\nexit 7\n",
    );
    let mut settings = provider_settings(
        AgentHarnessKind::Claude,
        AgentProviderCliManagementMode::UserManaged,
    );
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(claude_path.to_string_lossy().into_owned());

    let probe = provider_runtime_probe(&settings).expect("custom Claude probe");

    assert_eq!(
        probe.binary_path.as_deref(),
        Some(claude_path.to_string_lossy().as_ref())
    );
    assert!(probe.binary_found);
    assert!(!probe.probe_succeeded);
    assert!(!probe.available);
    assert!(probe
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("probe failed"));
}

#[test]
fn rx_managed_codex_runtime_probe_reports_missing_binary() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let missing_path = temp_dir.path().join("codex");
    let _override = override_managed_codex_binary_path_for_tests(missing_path.clone());
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::RxManaged,
    );

    let probe = provider_runtime_probe(&settings).expect("managed Codex probe");

    assert_eq!(
        probe.binary_path.as_deref(),
        Some(missing_path.to_string_lossy().as_ref())
    );
    assert!(!probe.binary_found);
    assert!(!probe.probe_succeeded);
    assert!(!probe.available);
    assert_eq!(
        probe.error.as_deref(),
        Some("RX-managed Codex is not installed.")
    );
}

#[test]
fn checked_rx_managed_codex_launch_path_rejects_missing_binary() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let missing_path = temp_dir.path().join("codex");
    let _override = override_managed_codex_binary_path_for_tests(missing_path);
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::RxManaged,
    );

    let result = checked_managed_provider_cli_launch_path(&settings, "test runtime")
        .expect("managed Codex launch path result");

    assert_eq!(
        result.expect_err("missing managed binary should fail"),
        "RX-managed Codex is not installed."
    );
}

#[test]
fn managed_probe_error_falls_back_to_purpose_and_provider() {
    let probe = HarnessRuntimeProbe {
        binary_path: None,
        binary_found: false,
        probe_succeeded: false,
        available: false,
        missing_core_exec_features: Vec::new(),
        cli_version: None,
        supported_model_aliases: None,
        supported_efforts: None,
        ultra_supported_models: Vec::new(),
        supports_fast_mode: false,
        fast_mode_supported_models: Vec::new(),
        error: None,
    };

    assert_eq!(
        managed_probe_error(probe, "test runtime", AgentHarnessKind::Codex),
        "test runtime harness unavailable: codex"
    );
}

#[test]
fn rx_managed_codex_runtime_probe_reports_probe_error() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex");
    write_executable(
        &codex_path,
        "#!/bin/sh\nprintf 'probe failed\\n' >&2\nexit 7\n",
    );
    let _override = override_managed_codex_binary_path_for_tests(codex_path.clone());
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::RxManaged,
    );

    let probe = provider_runtime_probe(&settings).expect("managed Codex probe");

    assert_eq!(
        probe.binary_path.as_deref(),
        Some(codex_path.to_string_lossy().as_ref())
    );
    assert!(probe.binary_found);
    assert!(!probe.probe_succeeded);
    assert!(!probe.available);
    assert!(probe
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("probe failed"));
}

#[test]
fn checked_rx_managed_codex_launch_path_accepts_available_cli() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex");
    write_modern_codex_cli(&codex_path);
    let _override = override_managed_codex_binary_path_for_tests(codex_path.clone());
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::RxManaged,
    );

    let result = checked_managed_provider_cli_launch_path(&settings, "test runtime")
        .expect("managed Codex launch path result");

    assert_eq!(result.expect("available managed binary"), codex_path);
}

#[test]
fn rx_managed_codex_runtime_probe_reports_missing_core_features() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex");
    write_executable(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.100.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Usage' 'Options:' '  --version'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Usage' 'Options:' '  --version'
  exit 2
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );
    let _override = override_managed_codex_binary_path_for_tests(codex_path);
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::RxManaged,
    );

    let probe = provider_runtime_probe(&settings).expect("managed Codex probe");

    assert!(probe.binary_found);
    assert!(probe.probe_succeeded);
    assert!(!probe.available);
    assert_eq!(probe.cli_version.as_deref(), Some("0.100.0"));
    assert!(probe
        .missing_core_exec_features
        .contains(&"exec_subcommand".to_string()));
    assert!(probe
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("exec_subcommand"));
}

#[test]
fn rx_managed_codex_runtime_probe_reports_available_modern_cli() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let codex_path = temp_dir.path().join("codex");
    write_modern_codex_cli(&codex_path);
    let _override = override_managed_codex_binary_path_for_tests(codex_path);
    let settings = provider_settings(
        AgentHarnessKind::Codex,
        AgentProviderCliManagementMode::RxManaged,
    );

    let probe = provider_runtime_probe(&settings).expect("managed Codex probe");

    assert!(probe.binary_found);
    assert!(probe.probe_succeeded);
    assert!(probe.available);
    assert_eq!(probe.cli_version.as_deref(), Some("0.144.0"));
    assert!(probe.missing_core_exec_features.is_empty());
    assert_eq!(
        probe.supported_model_aliases,
        Some(vec![
            "gpt-5.5".to_string(),
            "gpt-5.6-sol".to_string(),
            "gpt-5.6-terra".to_string(),
        ])
    );
    assert_eq!(
        probe.supported_efforts,
        Some(vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ])
    );
    assert_eq!(probe.error, None);
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
  printf '%s\n' '{"models":[{"slug":"gpt-5.5","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"}],"additional_speed_tiers":["fast"]},{"slug":"gpt-5.6-sol","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"},{"effort":"max"},{"effort":"ultra"}]},{"slug":"gpt-5.6-terra","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"},{"effort":"max"},{"effort":"ultra"}]}]}'
elif [ "$1" = "debug" ] && [ "$2" = "models" ] && [ "$3" = "--bundled" ]; then
  printf '%s\n' '{"models":[{"slug":"gpt-5.5","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"}],"additional_speed_tiers":["fast"]}]}'
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );
}

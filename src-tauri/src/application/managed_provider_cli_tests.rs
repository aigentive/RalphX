use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::utils::runtime_log_paths::{
    app_runtime_dir, managed_codex_binary_path, managed_provider_cli_dir,
};

use super::{
    managed_provider_cli_launch_path, managed_provider_runtime_probe,
    override_managed_codex_binary_path_for_tests,
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
    assert!(managed_provider_runtime_probe(&settings).is_none());
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

    let path = managed_provider_cli_launch_path(&settings)
        .expect("managed Codex path override")
        .expect("managed Codex path");

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
    assert!(managed_provider_runtime_probe(&settings).is_none());
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

    let probe = managed_provider_runtime_probe(&settings).expect("managed Codex probe");

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

    let probe = managed_provider_runtime_probe(&settings).expect("managed Codex probe");

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

    let probe = managed_provider_runtime_probe(&settings).expect("managed Codex probe");

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
    write_executable(
        &codex_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.144.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --search' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
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

    let probe = managed_provider_runtime_probe(&settings).expect("managed Codex probe");

    assert!(probe.binary_found);
    assert!(probe.probe_succeeded);
    assert!(probe.available);
    assert_eq!(probe.cli_version.as_deref(), Some("0.144.0"));
    assert!(probe.missing_core_exec_features.is_empty());
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

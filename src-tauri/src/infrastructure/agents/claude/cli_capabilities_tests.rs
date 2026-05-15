use crate::domain::agents::LogicalEffort;
use crate::infrastructure::agents::claude::cli_capabilities::{
    clear_claude_cli_capability_cache, normalize_claude_effort_for_capabilities,
    normalize_claude_effort_for_cli_path, parse_claude_cli_capabilities, parse_claude_version,
    probe_claude_cli_cached, ClaudeCliCapabilities,
};
use std::path::Path;

const MODERN_HELP: &str = r#"
Options:
  --effort <level>  Effort level for the current session (low, medium, high, xhigh, max)
"#;

const LEGACY_HELP: &str = r#"
Options:
  --effort <level>  Effort level for the current session (low, medium, high, max)
"#;

#[cfg(unix)]
fn write_fake_claude_cli(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write fake claude");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .expect("fake claude metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("mark fake claude executable");
}

#[test]
fn parse_claude_version_reads_native_version_output() {
    assert_eq!(
        parse_claude_version("2.1.142 (Claude Code)").as_deref(),
        Some("2.1.142")
    );
    assert_eq!(parse_claude_version("Claude Code dev build"), None);
}

#[test]
fn parse_capabilities_detects_modern_effort_surface_from_help() {
    let capabilities = parse_claude_cli_capabilities(MODERN_HELP, Some("2.1.142 (Claude Code)"));

    assert_eq!(capabilities.version.as_deref(), Some("2.1.142"));
    assert_eq!(
        capabilities.supported_efforts,
        vec![
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::XHigh,
            LogicalEffort::Max,
        ]
    );
}

#[test]
fn parse_capabilities_detects_legacy_effort_surface_from_help() {
    let capabilities = parse_claude_cli_capabilities(LEGACY_HELP, Some("2.1.110 (Claude Code)"));

    assert_eq!(capabilities.version.as_deref(), Some("2.1.110"));
    assert_eq!(
        capabilities.supported_efforts,
        vec![
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::Max,
        ]
    );
}

#[test]
fn parse_capabilities_falls_back_to_version_when_help_does_not_list_efforts() {
    let modern = parse_claude_cli_capabilities("", Some("2.1.111 (Claude Code)"));
    let legacy = parse_claude_cli_capabilities("", Some("2.1.110 (Claude Code)"));

    assert!(modern.supports_effort(LogicalEffort::XHigh));
    assert!(!legacy.supports_effort(LogicalEffort::XHigh));
}

#[test]
fn normalize_effort_keeps_supported_xhigh_and_max() {
    let capabilities = ClaudeCliCapabilities {
        version: Some("2.1.142".to_string()),
        supported_efforts: vec![
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::XHigh,
            LogicalEffort::Max,
        ],
    };

    assert_eq!(
        normalize_claude_effort_for_capabilities("xhigh", &capabilities),
        "xhigh"
    );
    assert_eq!(
        normalize_claude_effort_for_capabilities("max", &capabilities),
        "max"
    );
}

#[test]
fn normalize_effort_downgrades_xhigh_to_high_for_legacy_cli() {
    let capabilities = ClaudeCliCapabilities {
        version: Some("2.1.110".to_string()),
        supported_efforts: vec![
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::Max,
        ],
    };

    assert_eq!(
        normalize_claude_effort_for_capabilities("xhigh", &capabilities),
        "high"
    );
    assert_eq!(
        normalize_claude_effort_for_capabilities("max", &capabilities),
        "max"
    );
}

#[test]
fn normalize_effort_falls_back_for_invalid_or_over_requested_effort() {
    let capabilities = ClaudeCliCapabilities {
        version: Some("2.1.142".to_string()),
        supported_efforts: vec![LogicalEffort::High],
    };

    assert_eq!(
        normalize_claude_effort_for_capabilities("retired-effort", &capabilities),
        "medium"
    );
    assert_eq!(
        normalize_claude_effort_for_capabilities("low", &capabilities),
        "high"
    );
}

#[cfg(unix)]
#[test]
fn probe_claude_cli_cached_reads_help_version_and_reuses_cached_result() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_claude_cli_capability_cache();
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_claude_cli(
        &cli_path,
        r#"#!/bin/sh
case "$1" in
  --version)
    echo "2.1.142 (Claude Code)"
    ;;
  --help)
    echo "Options:"
    echo "  --effort <level>  Effort level for the current session (low, medium, high, xhigh, max)"
    ;;
  *)
    echo "unexpected $1" >&2
    exit 2
    ;;
esac
"#,
    );

    let first = probe_claude_cli_cached(&cli_path).expect("first probe should succeed");
    write_fake_claude_cli(
        &cli_path,
        r#"#!/bin/sh
echo "probe should be cached" >&2
exit 2
"#,
    );
    let second = probe_claude_cli_cached(&cli_path).expect("second probe should reuse cache");

    assert_eq!(first, second);
    assert_eq!(second.version.as_deref(), Some("2.1.142"));
    assert!(second.supports_effort(LogicalEffort::XHigh));

    clear_claude_cli_capability_cache();
}

#[cfg(unix)]
#[test]
fn normalize_effort_for_cli_path_uses_legacy_fallback_when_probe_fails() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_claude_cli_capability_cache();
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_claude_cli(
        &cli_path,
        r#"#!/bin/sh
echo "boom" >&2
exit 42
"#,
    );

    assert_eq!(
        normalize_claude_effort_for_cli_path(&cli_path, "xhigh"),
        "high"
    );

    clear_claude_cli_capability_cache();
}

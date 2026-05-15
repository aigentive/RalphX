use crate::domain::agents::LogicalEffort;
use crate::infrastructure::agents::claude::cli_capabilities::{
    normalize_claude_effort_for_capabilities, parse_claude_cli_capabilities, parse_claude_version,
    ClaudeCliCapabilities,
};

const MODERN_HELP: &str = r#"
Options:
  --effort <level>  Effort level for the current session (low, medium, high, xhigh, max)
"#;

const LEGACY_HELP: &str = r#"
Options:
  --effort <level>  Effort level for the current session (low, medium, high, max)
"#;

#[test]
fn parse_claude_version_reads_native_version_output() {
    assert_eq!(
        parse_claude_version("2.1.142 (Claude Code)").as_deref(),
        Some("2.1.142")
    );
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

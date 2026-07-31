use crate::domain::agents::LogicalEffort;
use crate::infrastructure::agents::claude::cli_capabilities::{
    clear_claude_cli_capability_cache, is_claude_fable_model, is_claude_opus_4_7_model,
    is_claude_opus_4_8_model, is_claude_opus_5_model, is_claude_sonnet_5_model,
    normalize_claude_effort_for_capabilities, normalize_claude_effort_for_cli_path,
    parse_claude_cli_capabilities, parse_claude_version, probe_claude_cli, probe_claude_cli_cached,
    validate_claude_model_for_cli_path, ClaudeCliCapabilities, CLAUDE_OPUS_4_7_API_MODEL_ID,
    CLAUDE_OPUS_4_8_API_MODEL_ID, CLAUDE_OPUS_5_API_MODEL_ID,
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
        capabilities.supported_model_aliases,
        vec!["sonnet", "opus", "haiku", CLAUDE_OPUS_4_7_API_MODEL_ID,]
    );
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

#[cfg(unix)]
#[test]
fn thinking_display_is_unsupported_when_help_omits_flag_even_if_version_short_circuits_unknown_args(
) {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_claude_cli_capability_cache();
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_claude_cli(
        &cli_path,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  printf '%s\n' 'Claude Code' 'Options:' '  --include-partial-messages'
  exit 0
fi
for arg in "$@"; do
  if [ "$arg" = "--version" ]; then
    printf 'claude-code 2.1.220\n'
    exit 0
  fi
done
printf "error: unknown option '%s'\n" "$1" >&2
exit 1
"#,
    );

    let capabilities = probe_claude_cli(&cli_path).expect("capability probe");

    assert!(capabilities.supports_include_partial_messages());
    assert!(
        !capabilities.supports_thinking_display(),
        "help text is authoritative; an unknown flag must not be inferred from a version-short-circuit"
    );
    clear_claude_cli_capability_cache();
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
fn parse_capabilities_enables_fable_alias_for_supported_cli_versions() {
    let before_fable = parse_claude_cli_capabilities("", Some("2.1.169 (Claude Code)"));
    let with_fable = parse_claude_cli_capabilities("", Some("2.1.170 (Claude Code)"));

    assert!(!before_fable.supports_fable_model());
    assert!(with_fable.supports_fable_model());
    assert_eq!(
        with_fable.supported_model_aliases,
        vec![
            "sonnet",
            "opus",
            "haiku",
            CLAUDE_OPUS_4_7_API_MODEL_ID,
            CLAUDE_OPUS_4_8_API_MODEL_ID,
            "fable",
        ]
    );
}

#[test]
fn parse_capabilities_enables_sonnet_5_for_supported_cli_versions() {
    let before_sonnet_5 = parse_claude_cli_capabilities("", Some("2.1.196 (Claude Code)"));
    let with_sonnet_5 = parse_claude_cli_capabilities("", Some("2.1.197 (Claude Code)"));

    assert!(!before_sonnet_5.supports_model_alias("claude-sonnet-5"));
    assert!(with_sonnet_5.supports_model_alias("claude-sonnet-5"));
    assert_eq!(
        with_sonnet_5.supported_model_aliases,
        vec![
            "sonnet",
            "opus",
            "haiku",
            CLAUDE_OPUS_4_7_API_MODEL_ID,
            CLAUDE_OPUS_4_8_API_MODEL_ID,
            "fable",
            "claude-sonnet-4-6",
            "claude-sonnet-5",
        ]
    );
}

#[test]
fn parse_capabilities_progressively_enables_pinned_opus_model_ids_at_each_floor() {
    let cases = [
        ("2.1.110", vec!["sonnet", "opus", "haiku"]),
        (
            "2.1.111",
            vec!["sonnet", "opus", "haiku", CLAUDE_OPUS_4_7_API_MODEL_ID],
        ),
        (
            "2.1.153",
            vec!["sonnet", "opus", "haiku", CLAUDE_OPUS_4_7_API_MODEL_ID],
        ),
        (
            "2.1.154",
            vec![
                "sonnet",
                "opus",
                "haiku",
                CLAUDE_OPUS_4_7_API_MODEL_ID,
                CLAUDE_OPUS_4_8_API_MODEL_ID,
            ],
        ),
        (
            "2.1.218",
            vec![
                "sonnet",
                "opus",
                "haiku",
                CLAUDE_OPUS_4_7_API_MODEL_ID,
                CLAUDE_OPUS_4_8_API_MODEL_ID,
                "fable",
                "claude-sonnet-4-6",
                "claude-sonnet-5",
            ],
        ),
        (
            "2.1.219",
            vec![
                "sonnet",
                "opus",
                "haiku",
                CLAUDE_OPUS_4_7_API_MODEL_ID,
                CLAUDE_OPUS_4_8_API_MODEL_ID,
                "fable",
                "claude-sonnet-4-6",
                "claude-sonnet-5",
                CLAUDE_OPUS_5_API_MODEL_ID,
            ],
        ),
    ];

    for (version, expected_aliases) in cases {
        let capabilities = parse_claude_cli_capabilities("", Some(version));
        assert_eq!(
            capabilities.supported_model_aliases,
            expected_aliases
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            "unexpected aliases for Claude Code {version}"
        );
    }
}

#[test]
fn fable_model_detection_accepts_alias_and_api_model_id() {
    assert!(is_claude_fable_model("fable"));
    assert!(is_claude_fable_model(" Claude-Fable-5 "));
    assert!(!is_claude_fable_model("opus"));
}

#[test]
fn sonnet_5_model_detection_accepts_api_model_id_only() {
    assert!(is_claude_sonnet_5_model(" Claude-Sonnet-5 "));
    assert!(!is_claude_sonnet_5_model("sonnet"));
    assert!(!is_claude_sonnet_5_model("claude-sonnet-4-6"));
}

#[test]
fn pinned_opus_model_detection_accepts_trimmed_case_normalized_exact_ids() {
    assert!(is_claude_opus_4_7_model(" Claude-Opus-4-7 "));
    assert!(is_claude_opus_4_8_model(" CLAUDE-OPUS-4-8 "));
    assert!(is_claude_opus_5_model(" claude-opus-5 "));
    assert!(!is_claude_opus_4_7_model("opus"));
    assert!(!is_claude_opus_4_8_model("claude-opus-4-7"));
    assert!(!is_claude_opus_5_model("claude-opus-4-8"));
}

#[test]
fn normalize_effort_keeps_supported_xhigh_and_max() {
    let capabilities = ClaudeCliCapabilities {
        version: Some("2.1.142".to_string()),
        supported_model_aliases: vec!["sonnet".to_string(), "opus".to_string()],
        supported_efforts: vec![
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::XHigh,
            LogicalEffort::Max,
        ],
        supports_include_partial_messages: false,
        supports_thinking_display: false,
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
        supported_model_aliases: vec!["sonnet".to_string(), "opus".to_string()],
        supported_efforts: vec![
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::Max,
        ],
        supports_include_partial_messages: false,
        supports_thinking_display: false,
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
        supported_model_aliases: vec!["sonnet".to_string()],
        supported_efforts: vec![LogicalEffort::High],
        supports_include_partial_messages: false,
        supports_thinking_display: false,
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
    assert!(!second.supports_fable_model());

    clear_claude_cli_capability_cache();
}

#[cfg(unix)]
#[test]
fn validate_fable_model_for_cli_path_requires_supported_version() {
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
    echo "2.1.169 (Claude Code)"
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

    let error = validate_claude_model_for_cli_path(&cli_path, "fable")
        .expect_err("old CLI should reject fable");
    assert!(error.contains("v2.1.170"));
    assert!(validate_claude_model_for_cli_path(&cli_path, "opus").is_ok());

    clear_claude_cli_capability_cache();
}

#[cfg(unix)]
#[test]
fn validate_fable_model_for_cli_path_accepts_supported_version() {
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
    echo "2.1.170 (Claude Code)"
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

    assert!(validate_claude_model_for_cli_path(&cli_path, "fable").is_ok());

    clear_claude_cli_capability_cache();
}

#[cfg(unix)]
#[test]
fn validate_sonnet_4_6_model_for_cli_path_requires_supported_version() {
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
    echo "2.1.196 (Claude Code)"
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

    let error = validate_claude_model_for_cli_path(&cli_path, "claude-sonnet-4-6")
        .expect_err("old CLI should reject Sonnet 4.6");
    assert!(error.contains("v2.1.197"));
    assert!(validate_claude_model_for_cli_path(&cli_path, "sonnet").is_ok());

    clear_claude_cli_capability_cache();
}

#[cfg(unix)]
#[test]
fn validate_sonnet_4_6_model_for_cli_path_accepts_supported_version() {
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
    echo "2.1.197 (Claude Code)"
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

    assert!(validate_claude_model_for_cli_path(&cli_path, "claude-sonnet-4-6").is_ok());

    clear_claude_cli_capability_cache();
}

#[cfg(unix)]
#[test]
fn validate_sonnet_5_model_for_cli_path_requires_supported_version() {
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
    echo "2.1.196 (Claude Code)"
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

    let error = validate_claude_model_for_cli_path(&cli_path, "claude-sonnet-5")
        .expect_err("old CLI should reject Sonnet 5");
    assert!(error.contains("v2.1.197"));
    assert!(validate_claude_model_for_cli_path(&cli_path, "sonnet").is_ok());

    clear_claude_cli_capability_cache();
}

#[cfg(unix)]
#[test]
fn validate_sonnet_5_model_for_cli_path_accepts_supported_version() {
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
    echo "2.1.197 (Claude Code)"
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

    assert!(validate_claude_model_for_cli_path(&cli_path, "claude-sonnet-5").is_ok());

    clear_claude_cli_capability_cache();
}

#[cfg(unix)]
#[test]
fn validate_pinned_opus_models_enforces_each_version_floor_with_specific_guidance() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp = tempfile::tempdir().expect("tempdir");
    let cases = [
        (
            CLAUDE_OPUS_4_7_API_MODEL_ID,
            "Claude Opus 4.7",
            "2.1.110",
            "2.1.111",
        ),
        (
            CLAUDE_OPUS_4_8_API_MODEL_ID,
            "Claude Opus 4.8",
            "2.1.153",
            "2.1.154",
        ),
        (
            CLAUDE_OPUS_5_API_MODEL_ID,
            "Claude Opus 5",
            "2.1.218",
            "2.1.219",
        ),
    ];

    for (model, display_name, unsupported_version, floor) in cases {
        clear_claude_cli_capability_cache();
        let cli_path = temp.path().join(model);
        write_fake_claude_cli(
            &cli_path,
            &format!(
                r#"#!/bin/sh
case "$1" in
  --version) echo "{unsupported_version} (Claude Code)" ;;
  --help) echo "Options:" ;;
  *) exit 2 ;;
esac
"#
            ),
        );

        let error = validate_claude_model_for_cli_path(&cli_path, model)
            .expect_err("CLI immediately below the floor should reject the pinned model");
        assert!(error.contains(display_name), "{error}");
        assert!(error.contains(&format!("v{floor}")), "{error}");
        assert!(error.contains(unsupported_version), "{error}");
        assert!(error.contains(model), "{error}");

        write_fake_claude_cli(
            &cli_path,
            &format!(
                r#"#!/bin/sh
case "$1" in
  --version) echo "{floor} (Claude Code)" ;;
  --help) echo "Options:" ;;
  *) exit 2 ;;
esac
"#
            ),
        );
        clear_claude_cli_capability_cache();
        let normalized = format!(" {} ", model.to_ascii_uppercase());
        assert!(
            validate_claude_model_for_cli_path(&cli_path, &normalized).is_ok(),
            "CLI at {floor} should accept {model}"
        );
    }

    clear_claude_cli_capability_cache();
}

#[cfg(unix)]
#[test]
fn validate_pinned_opus_models_fails_closed_when_capability_probe_fails() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_claude_cli(
        &cli_path,
        "#!/bin/sh\necho 'probe unavailable' >&2\nexit 42\n",
    );

    for (model, display_name, floor) in [
        (CLAUDE_OPUS_4_7_API_MODEL_ID, "Claude Opus 4.7", "2.1.111"),
        (CLAUDE_OPUS_4_8_API_MODEL_ID, "Claude Opus 4.8", "2.1.154"),
        (CLAUDE_OPUS_5_API_MODEL_ID, "Claude Opus 5", "2.1.219"),
    ] {
        clear_claude_cli_capability_cache();
        let error = validate_claude_model_for_cli_path(&cli_path, model)
            .expect_err("failed capability probe should reject pinned model selection");
        assert!(error.contains(display_name), "{error}");
        assert!(error.contains(&format!("v{floor}")), "{error}");
        assert!(error.contains(model), "{error}");
    }

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

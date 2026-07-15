use super::*;

#[test]
fn validation_purpose_string_roundtrip_and_unknown_fallback() {
    let cases = [
        ("baseline", ValidationPurpose::Baseline, "baseline"),
        ("wave_gate", ValidationPurpose::WaveGate, "wave_gate"),
        ("final", ValidationPurpose::Final, "final"),
        (
            "re_execution",
            ValidationPurpose::ReExecution,
            "re_execution",
        ),
        (
            "re-execution",
            ValidationPurpose::ReExecution,
            "re_execution",
        ),
        ("merge", ValidationPurpose::Merge, "merge"),
        ("custom", ValidationPurpose::Other, "other"),
    ];

    for (input, expected, stored) in cases {
        let parsed = ValidationPurpose::parse(input);
        assert_eq!(parsed, expected);
        assert_eq!(parsed.as_str(), stored);
    }
}

#[test]
fn validation_context_type_string_roundtrip_and_unknown_fallback() {
    let cases = [
        ("execution", ValidationContextType::Execution, "execution"),
        (
            "re_execution",
            ValidationContextType::ReExecution,
            "re_execution",
        ),
        (
            "re-execution",
            ValidationContextType::ReExecution,
            "re_execution",
        ),
        ("review", ValidationContextType::Review, "review"),
        (
            "agent_conversation",
            ValidationContextType::AgentConversation,
            "agent_conversation",
        ),
        (
            "agent-conversation",
            ValidationContextType::AgentConversation,
            "agent_conversation",
        ),
        ("merge", ValidationContextType::Merge, "merge"),
        ("custom", ValidationContextType::Unknown, "unknown"),
    ];

    for (input, expected, stored) in cases {
        let parsed = ValidationContextType::parse(input);
        assert_eq!(parsed, expected);
        assert_eq!(parsed.as_str(), stored);
    }
}

#[test]
fn validation_run_mode_string_roundtrip_and_default_reuse() {
    let cases = [
        (
            "reuse_or_run",
            ValidationRunMode::ReuseOrRun,
            "reuse_or_run",
        ),
        ("force", ValidationRunMode::Force, "force"),
        ("dry_run", ValidationRunMode::DryRun, "dry_run"),
        ("dry-run", ValidationRunMode::DryRun, "dry_run"),
        ("unknown", ValidationRunMode::ReuseOrRun, "reuse_or_run"),
    ];

    for (input, expected, stored) in cases {
        let parsed = ValidationRunMode::parse(input);
        assert_eq!(parsed, expected);
        assert_eq!(parsed.as_str(), stored);
    }
}

#[test]
fn validation_run_status_string_roundtrip_and_default_running() {
    let cases = [
        ("running", ValidationRunStatus::Running, "running"),
        ("passed", ValidationRunStatus::Passed, "passed"),
        ("failed", ValidationRunStatus::Failed, "failed"),
        ("error", ValidationRunStatus::Error, "error"),
        ("cancelled", ValidationRunStatus::Cancelled, "cancelled"),
        ("skipped", ValidationRunStatus::Skipped, "skipped"),
        ("unknown", ValidationRunStatus::Running, "running"),
    ];

    for (input, expected, stored) in cases {
        let parsed = ValidationRunStatus::parse(input);
        assert_eq!(parsed, expected);
        assert_eq!(parsed.as_str(), stored);
    }
}

#[test]
fn validation_command_metadata_string_roundtrips() {
    let source_cases = [
        (
            "project_analysis_ref",
            ValidationCommandSource::ProjectAnalysisRef,
            "project_analysis_ref",
        ),
        (
            "agent_selected",
            ValidationCommandSource::AgentSelected,
            "agent_selected",
        ),
        (
            "unknown",
            ValidationCommandSource::AgentSelected,
            "agent_selected",
        ),
    ];
    for (input, expected, stored) in source_cases {
        let parsed = ValidationCommandSource::parse(input);
        assert_eq!(parsed, expected);
        assert_eq!(parsed.as_str(), stored);
    }

    let category_cases = [
        ("test", ValidationCommandCategory::Test, "test"),
        ("lint", ValidationCommandCategory::Lint, "lint"),
        (
            "typecheck",
            ValidationCommandCategory::Typecheck,
            "typecheck",
        ),
        (
            "type_check",
            ValidationCommandCategory::Typecheck,
            "typecheck",
        ),
        ("build", ValidationCommandCategory::Build, "build"),
        ("format", ValidationCommandCategory::Format, "format"),
        ("custom", ValidationCommandCategory::Other, "other"),
    ];
    for (input, expected, stored) in category_cases {
        let parsed = ValidationCommandCategory::parse(input);
        assert_eq!(parsed, expected);
        assert_eq!(parsed.as_str(), stored);
    }
}

#[test]
fn validation_cache_and_command_status_string_roundtrips() {
    let cache_cases = [
        ("ran", ValidationCacheDecision::Ran, "ran"),
        ("cached", ValidationCacheDecision::Cached, "cached"),
        ("stale", ValidationCacheDecision::Stale, "stale"),
        ("forced", ValidationCacheDecision::Forced, "forced"),
        ("skipped", ValidationCacheDecision::Skipped, "skipped"),
        ("unknown", ValidationCacheDecision::Ran, "ran"),
    ];
    for (input, expected, stored) in cache_cases {
        let parsed = ValidationCacheDecision::parse(input);
        assert_eq!(parsed, expected);
        assert_eq!(parsed.as_str(), stored);
    }

    let status_cases = [
        ("passed", ValidationCommandStatus::Passed, "passed", true),
        ("failed", ValidationCommandStatus::Failed, "failed", false),
        ("error", ValidationCommandStatus::Error, "error", false),
        (
            "skipped",
            ValidationCommandStatus::Skipped,
            "skipped",
            false,
        ),
        ("cached", ValidationCommandStatus::Cached, "cached", true),
        ("unknown", ValidationCommandStatus::Passed, "passed", true),
    ];
    for (input, expected, stored, success_like) in status_cases {
        let parsed = ValidationCommandStatus::parse(input);
        assert_eq!(parsed, expected);
        assert_eq!(parsed.as_str(), stored);
        assert_eq!(parsed.is_success_like(), success_like);
    }
}

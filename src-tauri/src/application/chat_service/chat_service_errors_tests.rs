use super::{
    classify_codex_stream_failure, classify_provider_error_from_assistant_content,
    is_nonfatal_mcp_tool_cancellation, ProviderErrorCategory, StreamError,
};

#[test]
fn detects_user_cancelled_mcp_tool_call_variants() {
    assert!(is_nonfatal_mcp_tool_cancellation(
        "user cancelled MCP tool call"
    ));
    assert!(is_nonfatal_mcp_tool_cancellation(
        "Agent failed: user canceled mcp tool call"
    ));
    assert!(!is_nonfatal_mcp_tool_cancellation(
        "tool call failed: provider timeout"
    ));
}

#[test]
fn codex_local_command_failure_with_rate_limit_text_is_local_tool_failure() {
    let runtime_errors = Vec::<String>::new();
    let local_tool_errors = vec![
            "rg: src-tauri/src/domain/entities/agent_run.rs: No such file or directory\n\
             src-tauri/src/application/chat_service/chat_service_errors.rs: RateLimit => write!(f, \"rate_limit\")"
                .to_string(),
        ];

    let result = classify_codex_stream_failure(&runtime_errors, &local_tool_errors, Some(1), false)
        .expect("local command failure should surface as a local tool error");

    match result {
        StreamError::LocalToolFailed { message } => {
            assert!(message.contains("No such file or directory"));
            assert!(message.contains("rate_limit"));
        }
        other => {
            panic!("expected local Codex failure to become LocalToolFailed, got {other:?}")
        }
    }
}

#[test]
fn codex_mcp_tool_failure_with_rate_limit_text_is_local_tool_failure() {
    let runtime_errors = Vec::<String>::new();
    let local_tool_errors = vec![
            "delegate_start failed after reading provider_error category rate_limit from local metadata"
                .to_string(),
        ];

    let result = classify_codex_stream_failure(&runtime_errors, &local_tool_errors, Some(1), false)
        .expect("local MCP failure should surface as a local tool error");

    assert!(
        matches!(result, StreamError::LocalToolFailed { .. }),
        "local MCP failures must not become provider backpressure"
    );
}

#[test]
fn codex_runtime_rate_limit_error_still_classifies_as_provider_error() {
    let runtime_errors = vec!["Error: rate_limit_exceeded".to_string()];
    let local_tool_errors = Vec::<String>::new();

    let result = classify_codex_stream_failure(&runtime_errors, &local_tool_errors, Some(1), false)
        .expect("runtime provider failure should classify");

    match result {
        StreamError::ProviderError { category, .. } => {
            assert_eq!(category, ProviderErrorCategory::RateLimit);
        }
        other => panic!("expected provider error, got {other:?}"),
    }
}

#[test]
fn codex_split_runtime_provider_error_joins_runtime_messages() {
    let runtime_errors = vec!["429".to_string(), "Usage limit exceeded".to_string()];
    let local_tool_errors = Vec::<String>::new();

    let result = classify_codex_stream_failure(&runtime_errors, &local_tool_errors, Some(1), false)
        .expect("split runtime provider failure should classify");

    match result {
        StreamError::ProviderError { category, .. } => {
            assert_eq!(category, ProviderErrorCategory::RateLimit);
        }
        other => panic!("expected provider error, got {other:?}"),
    }
}

#[test]
fn codex_stream_failure_without_error_text_returns_none() {
    assert!(classify_codex_stream_failure(&[], &[], Some(0), false).is_none());
}

#[test]
fn assistant_content_rate_limit_literal_is_not_provider_error() {
    assert!(classify_provider_error_from_assistant_content(
        "The local metadata file contains the literal rate_limit string."
    )
    .is_none());
}

#[test]
fn assistant_content_claude_usage_limit_banner_stays_provider_error() {
    let result = classify_provider_error_from_assistant_content(
        "You've hit your limit. Your limit will reset at 2026-05-09 18:00:00",
    )
    .expect("Claude usage-limit banner should classify");

    match result {
        StreamError::ProviderError { category, .. } => {
            assert_eq!(category, ProviderErrorCategory::RateLimit);
        }
        other => panic!("expected provider rate limit, got {other:?}"),
    }
}

use ralphx_lib::application::chat_service::{
    classify_agent_error, classify_codex_stream_failure, classify_provider_error,
    parse_retry_after_from_message, truncate_error_message, PauseReason, ProviderErrorCategory,
    ProviderErrorMetadata, StreamError,
};
use ralphx_lib::domain::entities::{
    ChatContextType, ChatConversationId, ExecutionFailureSource, InternalStatus,
};
use ralphx_lib::error::AppError;

#[test]
fn test_classify_stale_session_error() {
    let error = "Error: No conversation found with session ID abc123";
    let conv_id = ChatConversationId::new();

    let classified = classify_agent_error(error, &conv_id, Some("abc123"));

    match classified {
        AppError::StaleSession { session_id, .. } => {
            assert_eq!(session_id, "abc123");
        }
        _ => panic!("Expected StaleSession error"),
    }
}

#[test]
fn test_classify_stale_session_without_stored_id() {
    let error = "Error: No conversation found with session ID abc123";
    let conv_id = ChatConversationId::new();

    let classified = classify_agent_error(error, &conv_id, None);

    match classified {
        AppError::Agent(_) => {} // Falls back to Agent error
        _ => panic!("Expected Agent error when no stored session ID"),
    }
}

#[test]
fn test_classify_other_error() {
    let error = "Rate limit exceeded";
    let conv_id = ChatConversationId::new();

    let classified = classify_agent_error(error, &conv_id, Some("abc123"));

    match classified {
        AppError::Agent(msg) => {
            assert_eq!(msg, "Rate limit exceeded");
        }
        _ => panic!("Expected Agent error for non-stale-session errors"),
    }
}

/// Regression test: Verify non-stale errors are NOT classified as StaleSession
/// This ensures the error path in chat_service_send_background.rs will NOT
/// trigger recovery for common errors like rate limits, network failures, etc.
#[test]
fn test_non_stale_errors_do_not_trigger_recovery() {
    let conversation_id = ChatConversationId::new();
    let test_cases = vec![
        "Rate limit exceeded",
        "Network timeout",
        "Command execution failed",
        "Invalid API key",
        "Permission denied",
        "Out of memory",
    ];

    for error_msg in test_cases {
        let result = classify_agent_error(error_msg, &conversation_id, Some("session-123"));

        match result {
            AppError::Agent(_) => {
                // Expected: non-stale errors should be classified as Agent errors
                // These will NOT match AppError::StaleSession pattern in error handling
            }
            AppError::StaleSession { .. } => {
                panic!(
                    "Error '{}' incorrectly classified as StaleSession. \
                     This would trigger unwanted recovery attempts.",
                    error_msg
                );
            }
            _ => {
                panic!("Unexpected error type for: {}", error_msg);
            }
        }
    }
}

// =========================================================================
// StreamError tests
// =========================================================================

#[test]
fn test_stream_error_is_retryable() {
    let retryable = vec![
        StreamError::Timeout {
            context_type: ChatContextType::TaskExecution,
            elapsed_secs: 600,
        },
        StreamError::ParseStall {
            context_type: ChatContextType::TaskExecution,
            elapsed_secs: 180,
            lines_seen: 100,
            lines_parsed: 0,
        },
        StreamError::SessionNotFound {
            session_id: "abc".to_string(),
        },
        StreamError::AgentExit {
            exit_code: Some(1),
            stderr: "error".to_string(),
        },
    ];

    for err in &retryable {
        assert!(err.is_retryable(), "{} should be retryable", err);
    }

    let not_retryable = vec![
        StreamError::ProcessSpawnFailed {
            command: "claude".to_string(),
            error: "not found".to_string(),
        },
        StreamError::NoOutput {
            context_type: ChatContextType::TaskExecution,
            exit_code: None,
            exit_signal: None,
            stderr: String::new(),
        },
        StreamError::Cancelled {
            turns_finalized: 0,
            completion_tool_called: false,
        },
    ];

    for err in &not_retryable {
        assert!(!err.is_retryable(), "{} should NOT be retryable", err);
    }
}

#[test]
fn test_stream_error_requires_session_clear() {
    let should_clear = vec![StreamError::SessionNotFound {
        session_id: "abc".to_string(),
    }];

    for err in &should_clear {
        assert!(
            err.requires_session_clear(),
            "{} should require session clear",
            err
        );
    }

    // Timeout and ParseStall are idle-based stalls — the provider session is
    // still valid and can be resumed. Clearing it would destroy continuity.
    let should_not_clear = vec![
        StreamError::Timeout {
            context_type: ChatContextType::TaskExecution,
            elapsed_secs: 600,
        },
        StreamError::ParseStall {
            context_type: ChatContextType::Review,
            elapsed_secs: 180,
            lines_seen: 50,
            lines_parsed: 0,
        },
        StreamError::AgentExit {
            exit_code: Some(1),
            stderr: "error".to_string(),
        },
        StreamError::ProcessSpawnFailed {
            command: "claude".to_string(),
            error: "not found".to_string(),
        },
        StreamError::NoOutput {
            context_type: ChatContextType::TaskExecution,
            exit_code: None,
            exit_signal: None,
            stderr: String::new(),
        },
        StreamError::Cancelled {
            turns_finalized: 0,
            completion_tool_called: false,
        },
    ];

    for err in &should_not_clear {
        assert!(
            !err.requires_session_clear(),
            "{} should NOT require session clear",
            err
        );
    }
}

#[test]
fn test_stream_error_suggested_task_status() {
    // Cancelled → Cancelled status
    assert_eq!(
        StreamError::Cancelled {
            turns_finalized: 0,
            completion_tool_called: false,
        }
        .suggested_task_status(),
        Some(InternalStatus::Cancelled)
    );

    // ProviderError → Paused status
    assert_eq!(
        StreamError::ProviderError {
            category: ProviderErrorCategory::RateLimit,
            message: "rate limited".to_string(),
            retry_after: None,
        }
        .suggested_task_status(),
        Some(InternalStatus::Paused)
    );

    // All other errors → Failed status
    let failed_errors = vec![
        StreamError::Timeout {
            context_type: ChatContextType::TaskExecution,
            elapsed_secs: 600,
        },
        StreamError::ParseStall {
            context_type: ChatContextType::TaskExecution,
            elapsed_secs: 180,
            lines_seen: 100,
            lines_parsed: 0,
        },
        StreamError::AgentExit {
            exit_code: Some(1),
            stderr: "error".to_string(),
        },
        StreamError::SessionNotFound {
            session_id: "abc".to_string(),
        },
        StreamError::ProcessSpawnFailed {
            command: "claude".to_string(),
            error: "not found".to_string(),
        },
        StreamError::NoOutput {
            context_type: ChatContextType::TaskExecution,
            exit_code: None,
            exit_signal: None,
            stderr: String::new(),
        },
    ];

    for err in &failed_errors {
        assert_eq!(
            err.suggested_task_status(),
            Some(InternalStatus::Failed),
            "{} should suggest Failed status",
            err
        );
    }
}

#[test]
fn test_stream_error_display() {
    let timeout = StreamError::Timeout {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 600,
    };
    assert!(timeout.to_string().contains("600s"));
    assert!(timeout.to_string().contains("task_execution"));

    let parse_stall = StreamError::ParseStall {
        context_type: ChatContextType::Review,
        elapsed_secs: 180,
        lines_seen: 50,
        lines_parsed: 5,
    };
    assert!(parse_stall.to_string().contains("180s"));
    assert!(parse_stall.to_string().contains("lines_seen=50"));

    let agent_exit = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "  some error  ".to_string(),
    };
    assert!(agent_exit.to_string().contains("some error"));

    let agent_exit_empty = StreamError::AgentExit {
        exit_code: Some(42),
        stderr: "".to_string(),
    };
    assert!(agent_exit_empty.to_string().contains("42"));

    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };
    assert!(cancelled.to_string().contains("cancelled"));

    let provider_err = StreamError::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "Usage limit reached".to_string(),
        retry_after: None,
    };
    assert!(provider_err.to_string().contains("rate_limit"));
    assert!(provider_err.to_string().contains("Usage limit reached"));
}

#[test]
fn test_agent_exit_summary_ignores_cargo_progress_noise() {
    let stderr = "\
   Compiling proc-macro2 v1.0.106
    Building [                           ] 0/108: proc-macro2(build.rs)
   Compiling unicode-ident v1.0.22
error: failed to write `/tmp/target/debug/.fingerprint/test-lib`

Caused by:
  No space left on device (os error 28)
";

    let summary = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: stderr.to_string(),
    }
    .to_string();

    assert!(summary.contains("failed to write"));
    assert!(summary.contains("No space left on device"));
    assert!(!summary.contains("Compiling proc-macro2"));
    assert!(!summary.contains("Building ["));
}

// =========================================================================
// ProviderError classification tests
// =========================================================================

#[test]
fn test_classify_429_rate_limit_zai_format() {
    let error = r#"429 {"error":{"code":"1308","message":"Usage limit reached for 5 hour. Your limit will reset at 2026-02-15 14:15:20"},"request_id":"20260215122056..."}"#;
    let result = classify_provider_error(error);
    assert!(result.is_some(), "Should classify 429 rate limit");
    let err = result.unwrap();
    assert!(matches!(
        err,
        StreamError::ProviderError {
            category: ProviderErrorCategory::RateLimit,
            ..
        }
    ));
    // Should parse retry_after
    if let StreamError::ProviderError { retry_after, .. } = &err {
        assert!(retry_after.is_some(), "Should parse retry_after timestamp");
        assert!(retry_after.as_ref().unwrap().contains("2026-02-15"));
    }
}

#[test]
fn test_classify_generic_rate_limit() {
    let result = classify_provider_error("Rate limit exceeded");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::RateLimit);
    }
}

#[test]
fn test_classify_too_many_requests() {
    let result = classify_provider_error("HTTP 429: Too Many Requests");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::RateLimit);
    }
}

#[test]
fn test_classify_overloaded() {
    let result = classify_provider_error("overloaded_error: API is overloaded");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::Overloaded);
    }
}

#[test]
fn test_classify_auth_error() {
    let result = classify_provider_error("401 Unauthorized: Invalid API key");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::AuthError);
    }
}

#[test]
fn test_classify_server_error() {
    let result = classify_provider_error("502 Bad Gateway");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::ServerError);
    }
}

#[test]
fn test_classify_network_error() {
    let result = classify_provider_error("Connection refused");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::NetworkError);
    }
}

#[test]
fn test_classify_api_timeout_network_error() {
    let result = classify_provider_error("API_TIMEOUT_MS=3000000ms, try increasing it");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::NetworkError);
    }
}

#[test]
fn test_classify_normal_error_not_provider() {
    let result = classify_provider_error("Build failed: compilation error on line 42");
    assert!(
        result.is_none(),
        "Normal errors should not be classified as provider errors"
    );
}

#[test]
fn test_classify_empty_string() {
    assert!(classify_provider_error("").is_none());
}

#[test]
fn test_codex_local_command_failure_with_rate_limit_text_is_local_tool_failure() {
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
        other => panic!("expected local Codex failure to become LocalToolFailed, got {other:?}"),
    }
}

#[test]
fn test_codex_mcp_tool_failure_with_rate_limit_text_is_local_tool_failure() {
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
fn test_codex_validation_failure_code_is_validation_failed() {
    let runtime_errors = Vec::<String>::new();
    let local_tool_errors =
        vec!["execute_run_task_validation rejected completion: validation_failed".to_string()];

    let result = classify_codex_stream_failure(&runtime_errors, &local_tool_errors, Some(1), false)
        .expect("validation rejection should classify");

    match result {
        StreamError::ValidationFailed { message } => {
            assert!(message.contains("execute_run_task_validation"));
            assert!(message.contains("validation_failed"));
        }
        other => panic!("expected validation failure, got {other:?}"),
    }
}

#[test]
fn test_codex_completed_turn_ignores_prior_validation_diagnostics() {
    let runtime_errors = Vec::<String>::new();
    let local_tool_errors =
        vec!["first validation attempt failed with validation_failed before repair".to_string()];

    let result = classify_codex_stream_failure(&runtime_errors, &local_tool_errors, Some(0), true);

    assert!(
        result.is_none(),
        "successful completion must not be reclassified from earlier validation diagnostics"
    );
}

#[test]
fn test_codex_runtime_error_with_local_diagnostics_is_agent_exit() {
    let runtime_errors = vec!["codex runtime process exited".to_string()];
    let local_tool_errors = vec!["latest local tool stderr".to_string()];

    let result = classify_codex_stream_failure(&runtime_errors, &local_tool_errors, Some(1), false)
        .expect("runtime process failure should classify");

    match result {
        StreamError::AgentExit { exit_code, stderr } => {
            assert_eq!(exit_code, Some(1));
            assert!(stderr.contains("codex runtime process exited"));
            assert!(stderr.contains("latest local tool stderr"));
        }
        other => panic!("expected agent exit, got {other:?}"),
    }
}

#[test]
fn test_codex_runtime_rate_limit_error_still_classifies_as_provider_error() {
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
fn test_provider_error_suggests_paused() {
    let err = StreamError::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "test".to_string(),
        retry_after: None,
    };
    assert_eq!(err.suggested_task_status(), Some(InternalStatus::Paused));
    assert!(err.is_provider_error());
    assert!(err.is_retryable());
    assert!(!err.requires_session_clear());
}

#[test]
fn test_provider_error_metadata_build() {
    let err = StreamError::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "Usage limit reached".to_string(),
        retry_after: Some("2026-02-15T14:15:20+00:00".to_string()),
    };
    let meta = err
        .provider_error_metadata(InternalStatus::Executing)
        .expect("Should build metadata");
    assert_eq!(meta.category, ProviderErrorCategory::RateLimit);
    assert_eq!(meta.previous_status, "executing");
    assert!(meta.auto_resumable);
    assert_eq!(meta.resume_attempts, 0);
}

// =========================================================================
// ProviderErrorMetadata tests
// =========================================================================

#[test]
fn test_provider_error_metadata_roundtrip() {
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "test".to_string(),
        retry_after: Some("2026-02-15T14:15:20+00:00".to_string()),
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:15:20+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 0,
    };

    let json = meta.write_to_task_metadata(None);
    let restored = ProviderErrorMetadata::from_task_metadata(Some(&json));
    assert!(restored.is_some());
    let restored = restored.unwrap();
    assert_eq!(restored.category, ProviderErrorCategory::RateLimit);
    assert_eq!(restored.previous_status, "executing");
}

#[test]
fn test_provider_error_metadata_preserves_existing() {
    let existing = r#"{"some_key": "some_value"}"#;
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::ServerError,
        message: "502".to_string(),
        retry_after: None,
        previous_status: "reviewing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 1,
    };

    let json = meta.write_to_task_metadata(Some(existing));
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("some_key").is_some(),
        "Should preserve existing metadata"
    );
    assert!(parsed.get("provider_error").is_some());
}

#[test]
fn test_provider_error_metadata_clear() {
    let with_error = r#"{"some_key": "val", "provider_error": {"category": "rate_limit"}}"#;
    let cleared = ProviderErrorMetadata::clear_from_task_metadata(Some(with_error));
    let parsed: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert!(
        parsed.get("provider_error").is_none(),
        "Should remove provider_error"
    );
    assert!(
        parsed.get("some_key").is_some(),
        "Should preserve other keys"
    );
}

#[test]
fn test_retry_eligibility_future_time() {
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "test".to_string(),
        retry_after: Some("2099-12-31T23:59:59+00:00".to_string()),
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 0,
    };
    assert!(
        !meta.is_retry_eligible(),
        "Should not be eligible with future retry_after"
    );
}

#[test]
fn test_retry_eligibility_past_time() {
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "test".to_string(),
        retry_after: Some("2020-01-01T00:00:00+00:00".to_string()),
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 0,
    };
    assert!(
        meta.is_retry_eligible(),
        "Should be eligible with past retry_after"
    );
}

#[test]
fn test_retry_eligibility_max_attempts_exceeded() {
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "test".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: ProviderErrorMetadata::max_resume_attempts(),
    };
    assert!(
        !meta.is_retry_eligible(),
        "Should not be eligible at max attempts"
    );
}

#[test]
fn test_retry_eligibility_not_auto_resumable() {
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::AuthError,
        message: "test".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: false,
        resume_attempts: 0,
    };
    assert!(
        !meta.is_retry_eligible(),
        "Should not be eligible when not auto_resumable"
    );
}

#[test]
fn test_parse_retry_after_from_message() {
    let msg = r#"Usage limit reached for 5 hour. Your limit will reset at 2026-02-15 14:15:20"#;
    let result = parse_retry_after_from_message(msg);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), "2026-02-15T14:15:20+00:00");
}

#[test]
fn test_parse_retry_after_no_match() {
    let result = parse_retry_after_from_message("Rate limit exceeded");
    assert!(result.is_none());
}

#[test]
fn test_provider_error_is_retryable() {
    let err = StreamError::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "test".to_string(),
        retry_after: None,
    };
    assert!(err.is_retryable());
}

// =========================================================================
// Additional classify_provider_error coverage
// =========================================================================

#[test]
fn test_classify_403_forbidden_auth_error() {
    let result = classify_provider_error("403 Forbidden: Access denied");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::AuthError);
    }
}

#[test]
fn test_classify_invalid_api_key_auth_error() {
    let result = classify_provider_error("Error: invalid_api_key");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::AuthError);
    }
}

#[test]
fn test_classify_500_internal_server_error() {
    let result = classify_provider_error("500 Internal Server Error");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::ServerError);
    }
}

#[test]
fn test_classify_503_service_unavailable() {
    let result = classify_provider_error("503 Service Unavailable");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::ServerError);
    }
}

#[test]
fn test_classify_504_gateway_timeout() {
    let result = classify_provider_error("504 Gateway Timeout");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::ServerError);
    }
}

#[test]
fn test_classify_connection_reset_network_error() {
    let result = classify_provider_error("Connection reset by peer");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::NetworkError);
    }
}

#[test]
fn test_classify_dns_resolution_failed_network_error() {
    let result = classify_provider_error("DNS resolution failed for api.anthropic.com");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::NetworkError);
    }
}

#[test]
fn test_classify_network_unreachable() {
    let result = classify_provider_error("Network is unreachable");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::NetworkError);
    }
}

// =========================================================================
// Additional ProviderErrorMetadata edge cases
// =========================================================================

#[test]
fn test_from_task_metadata_no_provider_error_key() {
    let metadata = r#"{"some_other_key": "value"}"#;
    let result = ProviderErrorMetadata::from_task_metadata(Some(metadata));
    assert!(
        result.is_none(),
        "Should return None when provider_error key is absent"
    );
}

#[test]
fn test_from_task_metadata_none_input() {
    let result = ProviderErrorMetadata::from_task_metadata(None);
    assert!(result.is_none(), "Should return None when metadata is None");
}

#[test]
fn test_from_task_metadata_corrupt_json() {
    let result = ProviderErrorMetadata::from_task_metadata(Some("not valid json {{{"));
    assert!(
        result.is_none(),
        "Should return None gracefully for corrupt JSON"
    );
}

#[test]
fn test_from_task_metadata_corrupt_provider_error_value() {
    let metadata = r#"{"provider_error": "not_an_object"}"#;
    let result = ProviderErrorMetadata::from_task_metadata(Some(metadata));
    assert!(
        result.is_none(),
        "Should return None when provider_error is not a valid object"
    );
}

// =========================================================================
// Additional StreamError integration tests
// =========================================================================

#[test]
fn test_agent_exit_is_not_provider_error() {
    let err = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "compilation failed".to_string(),
    };
    assert!(!err.is_provider_error());
}

#[test]
fn test_timeout_is_not_provider_error() {
    let err = StreamError::Timeout {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 600,
    };
    assert!(!err.is_provider_error());
}

#[test]
fn test_cancelled_is_not_provider_error() {
    assert!(!StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    }
    .is_provider_error());
}

#[test]
fn test_provider_error_metadata_returns_none_for_non_provider_variant() {
    let err = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "failed".to_string(),
    };
    assert!(
        err.provider_error_metadata(InternalStatus::Executing)
            .is_none(),
        "Non-ProviderError variants should return None"
    );
}

#[test]
fn test_provider_error_does_not_require_session_clear() {
    let err = StreamError::ProviderError {
        category: ProviderErrorCategory::ServerError,
        message: "502 Bad Gateway".to_string(),
        retry_after: None,
    };
    assert!(
        !err.requires_session_clear(),
        "ProviderError should NOT require session clear"
    );
}

#[test]
fn test_provider_error_metadata_roundtrip_all_fields() {
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::Overloaded,
        message: "API overloaded".to_string(),
        retry_after: Some("2026-12-31T23:59:59+00:00".to_string()),
        previous_status: "re_executing".to_string(),
        paused_at: "2026-02-15T10:00:00+00:00".to_string(),
        auto_resumable: false,
        resume_attempts: 3,
    };

    let json = meta.write_to_task_metadata(None);
    let restored = ProviderErrorMetadata::from_task_metadata(Some(&json)).unwrap();

    assert_eq!(restored.category, ProviderErrorCategory::Overloaded);
    assert_eq!(restored.message, "API overloaded");
    assert_eq!(
        restored.retry_after,
        Some("2026-12-31T23:59:59+00:00".to_string())
    );
    assert_eq!(restored.previous_status, "re_executing");
    assert_eq!(restored.paused_at, "2026-02-15T10:00:00+00:00");
    assert!(!restored.auto_resumable);
    assert_eq!(restored.resume_attempts, 3);
}

#[test]
fn test_retry_eligible_within_max_attempts_no_retry_after() {
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "test".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 2,
    };
    assert!(
        meta.is_retry_eligible(),
        "Should be eligible with attempts below max and no retry_after"
    );
}

#[test]
fn test_retry_eligible_at_max_minus_one() {
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "test".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: ProviderErrorMetadata::max_resume_attempts() - 1,
    };
    assert!(
        meta.is_retry_eligible(),
        "Should be eligible at MAX - 1 attempts"
    );
}

#[test]
fn test_clear_from_task_metadata_with_none() {
    let cleared = ProviderErrorMetadata::clear_from_task_metadata(None);
    let parsed: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert!(parsed.get("provider_error").is_none());
}

#[test]
fn test_classify_rate_limit_underscore_format() {
    let result = classify_provider_error("Error: rate_limit_exceeded");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::RateLimit);
    }
}

#[test]
fn test_classify_claude_limit_banner_as_rate_limit() {
    let result = classify_provider_error("You've hit your limit · resets 11pm (Europe/Bucharest)");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError {
        category,
        retry_after,
        ..
    }) = result
    {
        assert_eq!(category, ProviderErrorCategory::RateLimit);
        let parsed = retry_after
            .as_deref()
            .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok());
        assert!(
            parsed.is_some(),
            "Claude limit banner should produce retry_after"
        );
    }
}

#[test]
fn test_classify_claude_extra_usage_banner_as_rate_limit() {
    let result =
        classify_provider_error("You're out of extra usage · resets 8am (Europe/Bucharest)");
    assert!(result.is_some());
    if let Some(StreamError::ProviderError { category, .. }) = result {
        assert_eq!(category, ProviderErrorCategory::RateLimit);
    }
}

#[test]
fn test_classify_claude_session_limit_banner_as_rate_limit() {
    // Regression: the Claude session-window banner inserts "session"
    // ("You've hit your session limit"), which did not match the fixed
    // "you've hit your limit" prefix. It fell through to AgentExit and the
    // task was hard-failed instead of paused + auto-resumed.
    let result =
        classify_provider_error("You've hit your session limit · resets 1:20pm (Europe/Bucharest)");
    assert!(
        result.is_some(),
        "session-limit banner must classify as a provider error, got {result:?}"
    );
    match result {
        Some(StreamError::ProviderError {
            category,
            retry_after,
            ..
        }) => {
            assert_eq!(category, ProviderErrorCategory::RateLimit);
            let parsed = retry_after
                .as_deref()
                .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok());
            assert!(
                parsed.is_some(),
                "session-limit banner should produce retry_after"
            );
        }
        other => panic!("expected ProviderError(RateLimit), got {other:?}"),
    }
}

#[test]
fn test_parse_retry_after_from_claude_reset_banner_returns_future_timestamp() {
    let retry_after =
        parse_retry_after_from_message("You've hit your limit · resets 11pm (Europe/Bucharest)")
            .expect("Claude reset banner should parse");
    let parsed =
        chrono::DateTime::parse_from_rfc3339(&retry_after).expect("retry_after should be RFC3339");
    let now = chrono::Utc::now();
    assert!(
        parsed.with_timezone(&chrono::Utc) > now,
        "retry_after should be in the future"
    );
    assert!(
        parsed.with_timezone(&chrono::Utc) - now < chrono::Duration::hours(25),
        "retry_after should not be more than 25h away"
    );
}

// =========================================================================
// PauseReason tests
// =========================================================================

#[test]
fn test_pause_reason_user_initiated_roundtrip() {
    let reason = PauseReason::UserInitiated {
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        scope: "global".to_string(),
    };

    let json = reason.write_to_task_metadata(None);
    let restored = PauseReason::from_task_metadata(Some(&json));
    assert!(restored.is_some(), "Should restore UserInitiated");
    let restored = restored.unwrap();
    assert!(!restored.is_provider_error());
    assert_eq!(restored.previous_status(), "executing");
    match restored {
        PauseReason::UserInitiated { scope, .. } => assert_eq!(scope, "global"),
        _ => panic!("Expected UserInitiated"),
    }
}

#[test]
fn test_pause_reason_provider_error_roundtrip() {
    let reason = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "Usage limit reached".to_string(),
        retry_after: Some("2026-02-15T14:15:20+00:00".to_string()),
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 0,
    };

    let json = reason.write_to_task_metadata(None);
    let restored = PauseReason::from_task_metadata(Some(&json));
    assert!(restored.is_some(), "Should restore ProviderError");
    let restored = restored.unwrap();
    assert!(restored.is_provider_error());
    assert_eq!(restored.previous_status(), "executing");
    match restored {
        PauseReason::ProviderError {
            category,
            resume_attempts,
            ..
        } => {
            assert_eq!(category, ProviderErrorCategory::RateLimit);
            assert_eq!(resume_attempts, 0);
        }
        _ => panic!("Expected ProviderError"),
    }
}

#[test]
fn test_pause_reason_backward_compat_reads_old_provider_error_key() {
    // Simulate old-style metadata with only provider_error key
    let old_metadata = r#"{"provider_error":{"category":"rate_limit","message":"test","retry_after":null,"previous_status":"executing","paused_at":"2026-02-15T09:00:00+00:00","auto_resumable":true,"resume_attempts":1}}"#;

    let restored = PauseReason::from_task_metadata(Some(old_metadata));
    assert!(
        restored.is_some(),
        "Should read from legacy provider_error key"
    );
    let restored = restored.unwrap();
    assert!(restored.is_provider_error());
    assert_eq!(restored.previous_status(), "executing");
    match restored {
        PauseReason::ProviderError {
            category,
            resume_attempts,
            ..
        } => {
            assert_eq!(category, ProviderErrorCategory::RateLimit);
            assert_eq!(resume_attempts, 1);
        }
        _ => panic!("Expected ProviderError from legacy key"),
    }
}

#[test]
fn test_pause_reason_clear_removes_both_keys() {
    let metadata = r#"{"pause_reason":{"type":"user_initiated","previous_status":"executing","paused_at":"2026-02-15T09:00:00+00:00","scope":"global"},"provider_error":{"category":"rate_limit"},"other_key":"value"}"#;

    let cleared = PauseReason::clear_from_task_metadata(Some(metadata));
    let parsed: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert!(
        parsed.get("pause_reason").is_none(),
        "Should remove pause_reason"
    );
    assert!(
        parsed.get("provider_error").is_none(),
        "Should remove legacy provider_error"
    );
    assert!(
        parsed.get("other_key").is_some(),
        "Should preserve other keys"
    );
}

#[test]
fn test_pause_reason_clear_with_none() {
    let cleared = PauseReason::clear_from_task_metadata(None);
    let parsed: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert!(parsed.get("pause_reason").is_none());
    assert!(parsed.get("provider_error").is_none());
}

#[test]
fn test_pause_reason_preserves_existing_metadata() {
    let existing = r#"{"some_key": "value"}"#;
    let reason = PauseReason::UserInitiated {
        previous_status: "reviewing".to_string(),
        paused_at: "2026-02-15T10:00:00+00:00".to_string(),
        scope: "task".to_string(),
    };

    let json = reason.write_to_task_metadata(Some(existing));
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("some_key").is_some(),
        "Should preserve existing keys"
    );
    assert!(
        parsed.get("pause_reason").is_some(),
        "Should have pause_reason"
    );
}

#[test]
fn test_pause_reason_from_none_metadata() {
    assert!(PauseReason::from_task_metadata(None).is_none());
}

#[test]
fn test_pause_reason_from_corrupt_json() {
    assert!(PauseReason::from_task_metadata(Some("not valid json")).is_none());
}

#[test]
fn test_pause_reason_from_empty_object() {
    assert!(PauseReason::from_task_metadata(Some("{}")).is_none());
}

#[test]
fn test_pause_reason_per_task_scope() {
    let reason = PauseReason::UserInitiated {
        previous_status: "re_executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        scope: "task".to_string(),
    };

    let json = reason.write_to_task_metadata(None);
    let restored = PauseReason::from_task_metadata(Some(&json)).unwrap();
    match restored {
        PauseReason::UserInitiated {
            scope,
            previous_status,
            ..
        } => {
            assert_eq!(scope, "task");
            assert_eq!(previous_status, "re_executing");
        }
        _ => panic!("Expected UserInitiated"),
    }
}

// =========================================================================
// Resume metadata clearing tests
// =========================================================================

#[test]
fn test_clear_removes_both_pause_reason_and_provider_error() {
    // Simulate metadata with both keys (as written by handle_stream_error)
    let meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "limit".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 2,
    };
    let reason = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "limit".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 2,
    };

    let with_legacy = meta.write_to_task_metadata(Some(r#"{"custom":"data"}"#));
    let with_both = reason.write_to_task_metadata(Some(&with_legacy));

    // Verify both keys present
    let parsed: serde_json::Value = serde_json::from_str(&with_both).unwrap();
    assert!(parsed.get("provider_error").is_some());
    assert!(parsed.get("pause_reason").is_some());

    // Clear should remove both
    let cleared = PauseReason::clear_from_task_metadata(Some(&with_both));
    let parsed: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert!(
        parsed.get("provider_error").is_none(),
        "Should remove legacy key"
    );
    assert!(
        parsed.get("pause_reason").is_none(),
        "Should remove pause_reason key"
    );
    assert!(
        parsed.get("custom").is_some(),
        "Should preserve unrelated keys"
    );
}

// =========================================================================
// Resume attempts carry-forward tests
// =========================================================================

#[test]
fn test_provider_error_metadata_fresh_starts_at_zero() {
    let err = StreamError::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "limit".to_string(),
        retry_after: None,
    };
    let meta = err
        .provider_error_metadata(InternalStatus::Executing)
        .unwrap();
    assert_eq!(
        meta.resume_attempts, 0,
        "Fresh metadata should have 0 resume_attempts"
    );
}

#[test]
fn test_resume_attempts_can_be_carried_forward_via_metadata() {
    // Simulate existing metadata with resume_attempts = 3
    let existing = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "old".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 3,
    };
    let metadata_str = existing.write_to_task_metadata(None);

    // Read back and verify we can extract resume_attempts
    let restored = PauseReason::from_task_metadata(Some(&metadata_str)).unwrap();
    if let PauseReason::ProviderError {
        resume_attempts, ..
    } = restored
    {
        assert_eq!(resume_attempts, 3, "Should carry forward resume_attempts");
    } else {
        panic!("Expected ProviderError variant");
    }
}

#[test]
fn test_resume_attempts_carry_from_legacy_provider_error_key() {
    // Simulate legacy metadata with only provider_error key
    let legacy = ProviderErrorMetadata {
        category: ProviderErrorCategory::ServerError,
        message: "502".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 2,
    };
    let metadata_str = legacy.write_to_task_metadata(None);

    // PauseReason::from_task_metadata should read legacy key
    let restored = PauseReason::from_task_metadata(Some(&metadata_str)).unwrap();
    if let PauseReason::ProviderError {
        resume_attempts, ..
    } = restored
    {
        assert_eq!(resume_attempts, 2, "Should carry forward from legacy key");
    } else {
        panic!("Expected ProviderError variant from legacy key");
    }

    // Also verify ProviderErrorMetadata::from_task_metadata works
    let legacy_restored = ProviderErrorMetadata::from_task_metadata(Some(&metadata_str)).unwrap();
    assert_eq!(legacy_restored.resume_attempts, 2);
}

// =========================================================================
// Per-task resume previous_status tests
// =========================================================================

#[test]
fn test_pause_reason_previous_status_accessor() {
    let user = PauseReason::UserInitiated {
        previous_status: "reviewing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        scope: "task".to_string(),
    };
    assert_eq!(user.previous_status(), "reviewing");

    let provider = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "limit".to_string(),
        retry_after: None,
        previous_status: "merging".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 0,
    };
    assert_eq!(provider.previous_status(), "merging");
}

#[test]
fn test_pause_reason_previous_status_parses_to_internal_status() {
    let statuses = vec![
        ("executing", InternalStatus::Executing),
        ("re_executing", InternalStatus::ReExecuting),
        ("reviewing", InternalStatus::Reviewing),
        ("merging", InternalStatus::Merging),
        ("qa_refining", InternalStatus::QaRefining),
        ("qa_testing", InternalStatus::QaTesting),
    ];

    for (status_str, expected) in statuses {
        let reason = PauseReason::UserInitiated {
            previous_status: status_str.to_string(),
            paused_at: "2026-02-15T09:00:00+00:00".to_string(),
            scope: "task".to_string(),
        };
        let parsed: InternalStatus = reason.previous_status().parse().unwrap();
        assert_eq!(parsed, expected, "Failed to parse '{}'", status_str);
    }
}

#[test]
fn test_backward_compat_old_key_writes_always_use_new_key() {
    // Writing always uses the new pause_reason key
    let reason = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "limit".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 0,
    };
    let json_str = reason.write_to_task_metadata(None);
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(
        parsed.get("pause_reason").is_some(),
        "Should write pause_reason key"
    );
    assert!(
        parsed.get("provider_error").is_none(),
        "Should NOT write provider_error key"
    );
}

// =========================================================================
// Global pause stores UserInitiated with scope "global" on each task
// =========================================================================

#[test]
fn test_user_initiated_global_scope_metadata() {
    let reason = PauseReason::UserInitiated {
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        scope: "global".to_string(),
    };

    let json = reason.write_to_task_metadata(None);
    let restored = PauseReason::from_task_metadata(Some(&json)).unwrap();

    assert!(!restored.is_provider_error());
    match restored {
        PauseReason::UserInitiated {
            scope,
            previous_status,
            paused_at,
        } => {
            assert_eq!(scope, "global");
            assert_eq!(previous_status, "executing");
            assert_eq!(paused_at, "2026-02-15T09:00:00+00:00");
        }
        _ => panic!("Expected UserInitiated"),
    }
}

// =========================================================================
// ProviderError metadata stored correctly with all fields
// =========================================================================

#[test]
fn test_provider_error_pause_reason_all_fields_persist() {
    let reason = PauseReason::ProviderError {
        category: ProviderErrorCategory::Overloaded,
        message: "API overloaded, please try again".to_string(),
        retry_after: Some("2026-02-15T14:00:00+00:00".to_string()),
        previous_status: "re_executing".to_string(),
        paused_at: "2026-02-15T09:30:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 2,
    };

    let json = reason.write_to_task_metadata(Some(r#"{"existing":"data"}"#));
    let restored = PauseReason::from_task_metadata(Some(&json)).unwrap();

    assert!(restored.is_provider_error());
    match restored {
        PauseReason::ProviderError {
            category,
            message,
            retry_after,
            previous_status,
            paused_at,
            auto_resumable,
            resume_attempts,
        } => {
            assert_eq!(category, ProviderErrorCategory::Overloaded);
            assert_eq!(message, "API overloaded, please try again");
            assert_eq!(retry_after, Some("2026-02-15T14:00:00+00:00".to_string()));
            assert_eq!(previous_status, "re_executing");
            assert_eq!(paused_at, "2026-02-15T09:30:00+00:00");
            assert!(auto_resumable);
            assert_eq!(resume_attempts, 2);
        }
        _ => panic!("Expected ProviderError"),
    }

    // Verify existing metadata preserved
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.get("existing").unwrap().as_str().unwrap(), "data");
}

// =========================================================================
// Handle_stream_error writes both keys — verify dual-key read/clear
// =========================================================================

#[test]
fn test_dual_key_write_simulates_handle_stream_error() {
    // handle_stream_error writes both legacy provider_error AND new pause_reason
    let legacy_meta = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "Usage limit".to_string(),
        retry_after: Some("2026-02-15T14:00:00+00:00".to_string()),
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 1,
    };
    let pause_reason = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "Usage limit".to_string(),
        retry_after: Some("2026-02-15T14:00:00+00:00".to_string()),
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 1,
    };

    // Write both keys (as handle_stream_error does)
    let with_legacy = legacy_meta.write_to_task_metadata(None);
    let with_both = pause_reason.write_to_task_metadata(Some(&with_legacy));

    // Verify both keys present
    let parsed: serde_json::Value = serde_json::from_str(&with_both).unwrap();
    assert!(parsed.get("provider_error").is_some());
    assert!(parsed.get("pause_reason").is_some());

    // PauseReason::from_task_metadata should prefer pause_reason key
    let restored = PauseReason::from_task_metadata(Some(&with_both)).unwrap();
    assert!(restored.is_provider_error());
    assert_eq!(restored.previous_status(), "executing");

    // ProviderErrorMetadata::from_task_metadata should still work (legacy)
    let legacy_restored = ProviderErrorMetadata::from_task_metadata(Some(&with_both)).unwrap();
    assert_eq!(legacy_restored.resume_attempts, 1);

    // Clear should remove BOTH keys
    let cleared = PauseReason::clear_from_task_metadata(Some(&with_both));
    let parsed: serde_json::Value = serde_json::from_str(&cleared).unwrap();
    assert!(parsed.get("provider_error").is_none());
    assert!(parsed.get("pause_reason").is_none());
}

// =========================================================================
// Resume attempts increment across re-pause cycles
// =========================================================================

#[test]
fn test_resume_attempts_increment_across_cycles() {
    // Cycle 1: fresh provider error → resume_attempts = 0
    let cycle1 = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "limit".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 0,
    };
    let meta1 = cycle1.write_to_task_metadata(None);

    // Read back, increment, write as cycle 2
    let restored1 = PauseReason::from_task_metadata(Some(&meta1)).unwrap();
    let attempts1 = match &restored1 {
        PauseReason::ProviderError {
            resume_attempts, ..
        } => *resume_attempts,
        _ => panic!("Expected ProviderError"),
    };
    assert_eq!(attempts1, 0);

    let cycle2 = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "limit again".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T10:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: attempts1 + 1, // Carried forward + 1
    };
    let meta2 = cycle2.write_to_task_metadata(None);

    let restored2 = PauseReason::from_task_metadata(Some(&meta2)).unwrap();
    match restored2 {
        PauseReason::ProviderError {
            resume_attempts, ..
        } => {
            assert_eq!(resume_attempts, 1, "Should be 1 after first re-pause");
        }
        _ => panic!("Expected ProviderError"),
    }

    // Cycle 3: increment again
    let cycle3 = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "limit yet again".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T11:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 2,
    };
    let meta3 = cycle3.write_to_task_metadata(None);

    let restored3 = PauseReason::from_task_metadata(Some(&meta3)).unwrap();
    match restored3 {
        PauseReason::ProviderError {
            resume_attempts, ..
        } => {
            assert_eq!(resume_attempts, 2, "Should be 2 after second re-pause");
        }
        _ => panic!("Expected ProviderError"),
    }
}

// =========================================================================
// Resume task from various previous statuses
// =========================================================================

#[test]
fn test_pause_reason_roundtrip_for_all_agent_active_statuses() {
    let agent_active = vec![
        "executing",
        "re_executing",
        "reviewing",
        "merging",
        "qa_refining",
        "qa_testing",
    ];

    for status_str in agent_active {
        // UserInitiated from each status
        let user_reason = PauseReason::UserInitiated {
            previous_status: status_str.to_string(),
            paused_at: "2026-02-15T09:00:00+00:00".to_string(),
            scope: "task".to_string(),
        };
        let json = user_reason.write_to_task_metadata(None);
        let restored = PauseReason::from_task_metadata(Some(&json)).unwrap();
        assert_eq!(
            restored.previous_status(),
            status_str,
            "UserInitiated: previous_status mismatch for {}",
            status_str
        );
        let parsed: InternalStatus = restored.previous_status().parse().unwrap();
        assert!(
            !parsed.is_terminal(),
            "{} should not be terminal",
            status_str
        );

        // ProviderError from each status
        let provider_reason = PauseReason::ProviderError {
            category: ProviderErrorCategory::ServerError,
            message: "error".to_string(),
            retry_after: None,
            previous_status: status_str.to_string(),
            paused_at: "2026-02-15T09:00:00+00:00".to_string(),
            auto_resumable: true,
            resume_attempts: 0,
        };
        let json = provider_reason.write_to_task_metadata(None);
        let restored = PauseReason::from_task_metadata(Some(&json)).unwrap();
        assert_eq!(
            restored.previous_status(),
            status_str,
            "ProviderError: previous_status mismatch for {}",
            status_str
        );
    }
}

// =========================================================================
// Edge case: resume with no pause metadata falls back
// =========================================================================

#[test]
fn test_no_pause_metadata_returns_none() {
    // Empty metadata
    assert!(PauseReason::from_task_metadata(Some("{}")).is_none());
    // Metadata with unrelated keys
    assert!(PauseReason::from_task_metadata(Some(r#"{"stop_metadata":{}}"#)).is_none());
    // None input
    assert!(PauseReason::from_task_metadata(None).is_none());
}

// =========================================================================
// Pause already-paused task: metadata overwrite
// =========================================================================

#[test]
fn test_writing_pause_reason_overwrites_previous() {
    // First pause: UserInitiated from executing
    let first = PauseReason::UserInitiated {
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T09:00:00+00:00".to_string(),
        scope: "task".to_string(),
    };
    let json1 = first.write_to_task_metadata(None);

    // Second pause: overwrite with different reason
    let second = PauseReason::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "rate limited".to_string(),
        retry_after: None,
        previous_status: "executing".to_string(),
        paused_at: "2026-02-15T10:00:00+00:00".to_string(),
        auto_resumable: true,
        resume_attempts: 0,
    };
    let json2 = second.write_to_task_metadata(Some(&json1));

    // Should read the latest
    let restored = PauseReason::from_task_metadata(Some(&json2)).unwrap();
    assert!(
        restored.is_provider_error(),
        "Should read the latest PauseReason"
    );
}

// =========================================================================
// Truncation of error messages
// =========================================================================

#[test]
fn test_truncate_error_message_long() {
    let long_msg = "x".repeat(600);
    let truncated = truncate_error_message(&long_msg);
    assert_eq!(truncated.len(), 503); // 500 + "..."
    assert!(truncated.ends_with("..."));
}

#[test]
fn test_truncate_error_message_short() {
    let short = "short error";
    assert_eq!(truncate_error_message(short), "short error");
}

// ── to_execution_failure_source() classification tests (GAP for handle_stream_error) ──

#[test]
fn to_execution_failure_source_timeout_maps_to_transient_timeout() {
    let err = StreamError::Timeout {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 600,
    };
    assert_eq!(
        err.to_execution_failure_source(),
        ExecutionFailureSource::TransientTimeout
    );
}

#[test]
fn to_execution_failure_source_parse_stall_maps_to_parse_stall() {
    let err = StreamError::ParseStall {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 180,
        lines_seen: 10,
        lines_parsed: 0,
    };
    assert_eq!(
        err.to_execution_failure_source(),
        ExecutionFailureSource::ParseStall
    );
}

#[test]
fn to_execution_failure_source_agent_exit_maps_to_agent_crash() {
    let err = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "SIGKILL".into(),
    };
    assert_eq!(
        err.to_execution_failure_source(),
        ExecutionFailureSource::AgentCrash
    );
}

#[test]
fn to_execution_failure_source_local_tool_failed_maps_to_local_tool_failed() {
    let err = StreamError::LocalToolFailed {
        message: "local command failed".into(),
    };
    assert_eq!(
        err.to_execution_failure_source(),
        ExecutionFailureSource::LocalToolFailed
    );
    assert_eq!(err.to_string(), "Local tool failed: local command failed");
}

#[test]
fn to_execution_failure_source_validation_failed_maps_to_validation_failed() {
    let err = StreamError::ValidationFailed {
        message: "validation rejected completion".into(),
    };
    assert_eq!(
        err.to_execution_failure_source(),
        ExecutionFailureSource::ValidationFailed
    );
    assert_eq!(
        err.to_string(),
        "Validation failed: validation rejected completion"
    );
}

#[test]
fn to_execution_failure_source_provider_error_maps_to_unknown() {
    let err = StreamError::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "Rate limited".into(),
        retry_after: None,
    };
    // Provider errors are handled via Paused state, not execution recovery
    assert_eq!(
        err.to_execution_failure_source(),
        ExecutionFailureSource::Unknown
    );
}

#[test]
fn to_execution_failure_source_timeout_is_transient() {
    let err = StreamError::Timeout {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 600,
    };
    assert!(err.to_execution_failure_source().is_transient());
}

#[test]
fn to_execution_failure_source_parse_stall_is_transient() {
    let err = StreamError::ParseStall {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 180,
        lines_seen: 5,
        lines_parsed: 0,
    };
    assert!(err.to_execution_failure_source().is_transient());
}

#[test]
fn to_execution_failure_source_agent_exit_is_transient() {
    let err = StreamError::AgentExit {
        exit_code: None,
        stderr: String::new(),
    };
    assert!(err.to_execution_failure_source().is_transient());
}

#[test]
fn to_execution_failure_source_provider_error_is_not_transient() {
    let err = StreamError::ProviderError {
        category: ProviderErrorCategory::ServerError,
        message: "500".into(),
        retry_after: None,
    };
    assert!(!err.to_execution_failure_source().is_transient());
}

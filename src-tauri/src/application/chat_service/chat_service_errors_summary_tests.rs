use super::{classify_codex_stream_failure, StreamError, VALIDATION_FAILED_ERROR_CODE};
use crate::application::chat_service::ChatServiceError;
use crate::application::persona_resolver::PersonaError;
use crate::application::personas::PERSONA_UNAVAILABLE_PREFIX;
use crate::domain::entities::{ChatContextType, ExecutionFailureSource, InternalStatus};

#[test]
fn persona_unavailable_error_string_starts_with_named_constant() {
    let error: ChatServiceError = PersonaError::Unavailable {
        persona_id: "persona-1".to_string(),
    }
    .into();

    assert!(error.to_string().starts_with(PERSONA_UNAVAILABLE_PREFIX));
}

fn agent_exit_message(stderr: impl Into<String>) -> String {
    StreamError::AgentExit {
        exit_code: Some(1),
        stderr: stderr.into(),
    }
    .to_string()
}

#[test]
fn agent_exit_summary_returns_empty_for_whitespace_only_stderr() {
    assert_eq!(agent_exit_message(" \n\t\r "), "Agent failed: ");
}

#[test]
fn agent_exit_summary_keeps_short_stderr_when_no_noise_was_removed() {
    assert_eq!(
        agent_exit_message("  fatal: direct failure  "),
        "Agent failed: fatal: direct failure"
    );
}

#[test]
fn agent_exit_summary_scores_error_lines_after_progress_noise_removed() {
    let stderr = "\
Compiling ralphx v0.1.0
building [====>                  ] 12/40
running 12 tests
pass [   1/12] unrelated::test
duration 3.1s
note: run with `RUST_BACKTRACE=1`
invalid ignored mode '--ignored=path'
no such file or directory: src/missing.rs
permission denied opening target/debug
Caused by: failed to compile dependency
failed to write target/debug/.fingerprint/output
12 test files failed
7 tests failed
fail suite::case
failures:
test result: FAILED. 1 passed; 1 failed
assertion `left == right` failed
panicked at src/lib.rs:42:9
error: could not compile crate
fatal: not a git repository
AssertionError: expected true
failed: command exited with code 1
received: null
expected: value
";

    let summary = agent_exit_message(stderr);

    assert!(summary.contains("invalid ignored mode"));
    assert!(summary.contains("permission denied"));
    assert!(summary.contains("no such file or directory"));
    assert!(summary.contains("failed to write"));
    assert!(!summary.contains("Compiling ralphx"));
    assert!(!summary.contains("pass ["));
}

#[test]
fn agent_exit_summary_truncates_long_unranked_output() {
    let stderr = "context line without recognized severity\n".repeat(80);

    let summary = agent_exit_message(&stderr);

    assert!(summary.ends_with("..."));
    assert!(summary.len() < stderr.len());
}

#[test]
fn agent_exit_summary_preserves_short_unranked_multiline_output() {
    let stderr = (0..10)
        .map(|index| format!("neutral context line {index}"))
        .collect::<Vec<_>>()
        .join("\n");

    let summary = agent_exit_message(stderr.clone());

    assert_eq!(summary, format!("Agent failed: {stderr}"));
}

#[test]
fn local_tool_failed_error_reports_failed_status_and_source() {
    let err = StreamError::LocalToolFailed {
        message: "local command failed".to_string(),
    };

    assert_eq!(err.to_string(), "Local tool failed: local command failed");
    assert_eq!(err.suggested_task_status(), Some(InternalStatus::Failed));
    assert_eq!(
        err.to_execution_failure_source(),
        ExecutionFailureSource::LocalToolFailed
    );
}

#[test]
fn validation_failed_error_reports_failed_status_and_source() {
    let err = StreamError::ValidationFailed {
        message: "validation rejected completion".to_string(),
    };

    assert_eq!(
        err.to_string(),
        "Validation failed: validation rejected completion"
    );
    assert_eq!(err.suggested_task_status(), Some(InternalStatus::Failed));
    assert_eq!(
        err.to_execution_failure_source(),
        ExecutionFailureSource::ValidationFailed
    );
}

#[test]
fn codex_completed_turn_suppresses_prior_local_tool_diagnostics() {
    let result = classify_codex_stream_failure(
        &[],
        &[format!(
            "earlier execute_run_task_validation diagnostic: {VALIDATION_FAILED_ERROR_CODE}"
        )],
        Some(0),
        true,
    );

    assert!(
        result.is_none(),
        "successful completion must not turn prior local diagnostics into a failure"
    );
}

#[test]
fn codex_validation_failure_code_returns_validation_failed() {
    let result = classify_codex_stream_failure(
        &[],
        &[format!(
            "execute_run_task_validation rejected: {VALIDATION_FAILED_ERROR_CODE}"
        )],
        Some(1),
        false,
    )
    .expect("validation failure should classify");

    match result {
        StreamError::ValidationFailed { message } => {
            assert!(message.contains("execute_run_task_validation"));
            assert!(message.contains(VALIDATION_FAILED_ERROR_CODE));
        }
        other => panic!("expected validation failure, got {other:?}"),
    }
}

#[test]
fn codex_runtime_error_with_local_diagnostics_returns_agent_exit() {
    let result = classify_codex_stream_failure(
        &["codex runtime exited".to_string()],
        &["local command stderr".to_string()],
        Some(1),
        false,
    )
    .expect("runtime failure should classify");

    match result {
        StreamError::AgentExit { exit_code, stderr } => {
            assert_eq!(exit_code, Some(1));
            assert!(stderr.contains("codex runtime exited"));
            assert!(stderr.contains("local command stderr"));
        }
        other => panic!("expected agent exit, got {other:?}"),
    }
}

#[test]
fn codex_local_diagnostics_without_runtime_error_returns_local_tool_failed() {
    let result =
        classify_codex_stream_failure(&[], &["local MCP tool failed".to_string()], Some(1), false)
            .expect("local tool failure should classify");

    match result {
        StreamError::LocalToolFailed { message } => {
            // The terminal fact is that the stream ended without completing; the
            // tool diagnostic is retained as supporting evidence behind it.
            assert_eq!(
                message,
                "Codex stream ended without a completion signal; \
                 local tool diagnostics from this turn: local MCP tool failed"
            );
        }
        other => panic!("expected local tool failure, got {other:?}"),
    }
}

#[test]
fn codex_stdin_notice_is_progress_noise_not_a_terminal_cause() {
    let result = classify_codex_stream_failure(
        &["Reading additional input from stdin...".to_string()],
        &[],
        Some(1),
        false,
    );

    assert!(result.is_none());
}

#[test]
fn codex_stdin_notice_does_not_mask_actionable_runtime_error() {
    let result = classify_codex_stream_failure(
        &[
            "Reading additional input from stdin...".to_string(),
            "fatal: provider process crashed".to_string(),
        ],
        &[],
        Some(1),
        false,
    )
    .expect("actionable runtime failure");

    match result {
        StreamError::AgentExit { stderr, .. } => {
            assert_eq!(stderr, "fatal: provider process crashed");
        }
        other => panic!("expected agent exit, got {other:?}"),
    }
}

#[test]
fn no_output_error_has_actionable_summary_and_retains_terminal_details() {
    let error = StreamError::NoOutput {
        context_type: ChatContextType::Project,
        exit_code: Some(1),
        exit_signal: None,
        stderr: "Reading additional input from stdin...".to_string(),
    }
    .to_string();

    // The delegation UI keys off this prefix to render the typed
    // "Delegate completed without a response" cause
    // (frontend/src/components/Chat/delegation-tool-calls.ts). Changing it
    // requires updating that mapping in the same change.
    assert!(error.starts_with("Codex exited without a response"));
    assert!(error.contains("code=Some(1)"));
    assert!(error.contains("Reading additional input from stdin"));
}

#[test]
fn timeout_and_agent_exit_status_mapping_remains_failed() {
    let timeout = StreamError::Timeout {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 10,
    };
    let agent_exit = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "runtime exited".to_string(),
    };

    assert_eq!(
        timeout.suggested_task_status(),
        Some(InternalStatus::Failed)
    );
    assert_eq!(
        agent_exit.suggested_task_status(),
        Some(InternalStatus::Failed)
    );
}

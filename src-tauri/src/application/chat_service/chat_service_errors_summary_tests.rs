use super::StreamError;

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

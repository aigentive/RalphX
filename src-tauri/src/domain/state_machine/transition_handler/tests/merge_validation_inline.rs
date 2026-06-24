// Tests extracted from merge_validation.rs #[cfg(test)] mod tests
//
// Covers: INSTALL_RETRY_DELAY_MS constant, run_install_phase retry behavior
//
// NOTE: These tests access private items from merge_validation.rs
// (INSTALL_RETRY_DELAY_MS, run_install_phase, PreExecAnalysisEntry).
// The source module's visibility must be adjusted to pub(super) or pub(crate)
// for these tests to compile from the external test file.

use super::super::merge_validation::{
    run_install_phase, PreExecAnalysisEntry, INSTALL_RETRY_DELAY_MS,
};

/// INSTALL_RETRY_DELAY_MS must be 500ms — covers macOS filesystem lock window
/// (Spotlight indexing, npm ENOTEMPTY) while cutting the original 2s delay by 75%.
#[test]
fn install_retry_delay_is_500ms() {
    assert_eq!(INSTALL_RETRY_DELAY_MS, 500);
}

/// run_install_phase retries once on failure and reports success when retry succeeds.
/// Uses a flag file: first call exits 1 (simulates transient ENOTEMPTY), second exits 0.
/// The retry overwrites the log entry in-place, so one entry with status "success" is recorded.
#[tokio::test]
async fn install_retry_succeeds_after_transient_failure() {
    let dir = tempfile::tempdir().unwrap();

    // first call: if flag exists, remove it and exit 1; second call: flag absent, exit 0.
    let flag = dir.path().join("fail_flag");
    std::fs::write(&flag, "").unwrap();

    let flag_path = flag.to_string_lossy().to_string();
    let cmd = format!(
        "if [ -f '{flag}' ]; then rm '{flag}'; exit 1; else exit 0; fi",
        flag = flag_path
    );

    let entries = vec![PreExecAnalysisEntry {
        path: ".".to_string(),
        label: "Test".to_string(),
        install: Some(cmd),
        worktree_setup: vec![],
    }];

    let cancel = tokio_util::sync::CancellationToken::new();
    let (log, had_failures) = run_install_phase(
        &entries,
        dir.path(),
        "test-task-id",
        None,
        &|s: &str| s.to_string(),
        "test",
        &cancel,
    )
    .await;

    // Retry succeeded → no failures overall; log has one entry replaced with "success"
    assert!(!had_failures, "expected no failures after successful retry");
    assert_eq!(log.len(), 1, "expected one log entry per command");
    assert_eq!(
        log[0].status, "success",
        "retry should have overwritten status to success"
    );
}

#[tokio::test]
async fn install_is_skipped_when_node_modules_exists() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("node_modules")).unwrap();

    let entries = vec![PreExecAnalysisEntry {
        path: ".".to_string(),
        label: "Test".to_string(),
        install: Some("echo should-not-run".to_string()),
        worktree_setup: vec![],
    }];

    let cancel = tokio_util::sync::CancellationToken::new();
    let (log, had_failures) = run_install_phase(
        &entries,
        dir.path(),
        "test-task-id",
        None,
        &|s: &str| s.to_string(),
        "test",
        &cancel,
    )
    .await;

    assert!(!had_failures, "skipped setup should not mark failures");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].status, "skipped");
    assert_eq!(log[0].stderr, "node_modules already exists — install skipped");
}

#[tokio::test]
async fn install_captures_cancelled_commands_as_failures() {
    let dir = tempfile::tempdir().unwrap();

    let entries = vec![PreExecAnalysisEntry {
        path: ".".to_string(),
        label: "Test".to_string(),
        install: Some("sleep 2".to_string()),
        worktree_setup: vec![],
    }];

    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();

    let (log, had_failures) = run_install_phase(
        &entries,
        dir.path(),
        "test-task-id",
        None,
        &|s: &str| s.to_string(),
        "test",
        &cancel,
    )
    .await;

    assert!(had_failures, "cancelled command should mark failure");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].status, "failed");
    assert_eq!(log[0].stderr, "Command cancelled");
}

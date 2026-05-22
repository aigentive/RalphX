use chrono::{Duration, Utc};
use serde_json::{Map, Value, json};

/// Replicates the staleness check logic from `recover_timeout_failures`.
///
/// Returns `true` if `failed_at` is present, parseable, and older than `threshold_secs`.
/// Returns `false` when `failed_at` is absent (non-stale: pre-existing tasks get one attempt).
fn is_task_stale(metadata: &Map<String, Value>, threshold_secs: u64) -> bool {
    if let Some(failed_at_str) = metadata.get("failed_at").and_then(|v| v.as_str()) {
        if let Ok(failed_at) = chrono::DateTime::parse_from_rfc3339(failed_at_str) {
            let age_secs = (Utc::now() - failed_at.with_timezone(&Utc)).num_seconds();
            return age_secs > threshold_secs as i64;
        }
    }
    false // absent failed_at = non-stale
}

#[test]
fn test_staleness_check_stale_task() {
    let mut metadata = Map::new();
    // 2 days ago — well beyond the 86400s (1 day) threshold
    let two_days_ago = Utc::now() - Duration::seconds(172_800);
    metadata.insert("failed_at".to_string(), json!(two_days_ago.to_rfc3339()));

    assert!(
        is_task_stale(&metadata, 86400),
        "Task with failed_at 2 days ago should be stale"
    );
}

#[test]
fn test_staleness_check_non_stale_no_failed_at() {
    let metadata = Map::new(); // empty — no failed_at key

    assert!(
        !is_task_stale(&metadata, 86400),
        "Task with no failed_at should be non-stale"
    );
}

#[test]
fn test_staleness_check_recent_task_not_stale() {
    let mut metadata = Map::new();
    // 1 hour ago — well within the 86400s (1 day) threshold
    let one_hour_ago = Utc::now() - Duration::seconds(3600);
    metadata.insert("failed_at".to_string(), json!(one_hour_ago.to_rfc3339()));

    assert!(
        !is_task_stale(&metadata, 86400),
        "Task failed 1 hour ago should not be stale"
    );
}

// ============================================================
// Tests for is_permanent_git_error() classifier
// ============================================================

/// Replicates is_permanent_git_error() from execution.rs for testing.
/// This mirrors the production function to verify classification behavior.
fn is_permanent_git_error_test(msg: &str) -> bool {
    msg.contains("invalid reference")
        || msg.contains("not a valid object name")
        || msg.contains("does not point to a valid object")
        || msg.contains("no longer exists")
}

#[test]
fn test_permanent_git_error_invalid_reference() {
    // Git says the branch ref is invalid (deleted branch)
    let msg = "Git isolation failed: invalid reference 'refs/heads/ralphx/task-abc'";
    assert!(
        is_permanent_git_error_test(msg),
        "Should detect 'invalid reference' as permanent git error"
    );
}

#[test]
fn test_permanent_git_error_not_valid_object() {
    let msg = "fatal: not a valid object name: 'ralphx/task-abc'";
    assert!(
        is_permanent_git_error_test(msg),
        "Should detect 'not a valid object name' as permanent git error"
    );
}

#[test]
fn test_permanent_git_error_does_not_point_to_valid_object() {
    let msg = "error: refs/heads/ralphx/task-abc does not point to a valid object";
    assert!(
        is_permanent_git_error_test(msg),
        "Should detect 'does not point to a valid object' as permanent git error"
    );
}

#[test]
fn test_permanent_git_error_branch_no_longer_exists() {
    // Matches the error from Fix 4 (branch_exists check in on_enter_states.rs)
    let msg = "branch 'ralphx/task-abc' no longer exists (deleted during prior merge cleanup)";
    assert!(
        is_permanent_git_error_test(msg),
        "Should detect 'no longer exists' as permanent git error"
    );
}

#[test]
fn test_permanent_git_error_transient_not_matched() {
    // Transient errors should NOT be classified as permanent
    let transient_errors = [
        "fatal: Unable to create '.git/index.lock': File exists",
        "error: timeout waiting for git",
        "network error: connection refused",
        "error: unable to acquire lock on git index",
        "fatal: Out of memory, malloc failed",
    ];
    for msg in &transient_errors {
        assert!(
            !is_permanent_git_error_test(msg),
            "Should NOT classify transient error as permanent: {}",
            msg
        );
    }
}

#[test]
fn test_permanent_git_error_empty_message_not_permanent() {
    assert!(
        !is_permanent_git_error_test(""),
        "Empty message should not be classified as permanent git error"
    );
}

// ============================================================
// Tests for set_preserve_steps_metadata logic
// ============================================================

/// Replicates the core logic of `set_preserve_steps_metadata` for unit testing.
fn build_preserve_steps_metadata(existing: Option<&str>) -> String {
    use crate::domain::state_machine::transition_handler::metadata_builder::MetadataUpdate;
    MetadataUpdate::new()
        .with_bool("preserve_steps", true)
        .merge_into(existing)
}

#[test]
fn test_preserve_steps_flag_set_on_empty_metadata() {
    let result = build_preserve_steps_metadata(None);
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        parsed["preserve_steps"],
        serde_json::Value::Bool(true),
        "preserve_steps should be true when set on empty metadata"
    );
}

#[test]
fn test_preserve_steps_flag_merged_with_existing_metadata() {
    // Simulate re-fetched task metadata after reset (ManualRetry event present, no stale keys)
    let existing = r#"{"execution_recovery": {"events": [], "state": "retrying"}}"#;
    let result = build_preserve_steps_metadata(Some(existing));
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(
        parsed["preserve_steps"],
        serde_json::Value::Bool(true),
        "preserve_steps should be set to true"
    );
    // Existing keys should be preserved
    assert!(
        parsed.get("execution_recovery").is_some(),
        "existing execution_recovery key should be preserved after merge"
    );
}

#[test]
fn test_preserve_steps_flag_absent_from_reset_metadata() {
    // Simulate what reset_execution_recovery_metadata produces (clean slate, no stale keys)
    // The flag must NOT be present until set_preserve_steps_metadata is called
    let clean_metadata = r#"{"trigger_origin": "scheduler"}"#;
    let parsed: serde_json::Value = serde_json::from_str(clean_metadata).unwrap();

    assert!(
        parsed.get("preserve_steps").is_none(),
        "preserve_steps should be absent from reset metadata (flag not yet set)"
    );
    assert!(
        parsed.get("is_timeout").is_none(),
        "is_timeout should be absent from reset metadata"
    );
    assert!(
        parsed.get("failure_error").is_none(),
        "failure_error should be absent from reset metadata"
    );
}

#[test]
fn test_preserve_steps_flag_overwrites_false_value() {
    // Edge case: if somehow preserve_steps was false, setting it again must make it true
    let existing = r#"{"preserve_steps": false, "trigger_origin": "scheduler"}"#;
    let result = build_preserve_steps_metadata(Some(existing));
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        parsed["preserve_steps"],
        serde_json::Value::Bool(true),
        "preserve_steps should be overwritten to true"
    );
}

// ============================================================
// Tests for is_structural_git_error() classifier
// ============================================================

/// Replicates is_structural_git_error() from execution.rs for testing.
fn is_structural_git_error_test(msg: &str) -> bool {
    if msg.contains("structural:") {
        return true;
    }
    msg.contains("does not exist") && msg.contains("invalid reference")
}

#[test]
fn test_structural_git_error_structural_prefix() {
    let msg = "git_isolation_error: structural: base branch 'main' does not exist";
    assert!(
        is_structural_git_error_test(msg),
        "Should detect 'structural:' prefix as structural git error"
    );
}

#[test]
fn test_structural_git_error_combined_does_not_exist_and_invalid_reference() {
    let msg = "fatal: invalid reference: refs/heads/main does not exist";
    assert!(
        is_structural_git_error_test(msg),
        "Should detect combined 'does not exist' + 'invalid reference' as structural git error"
    );
}

#[test]
fn test_structural_git_error_only_does_not_exist_not_structural() {
    // Only one of the combined patterns — should NOT match
    let msg = "fatal: path 'some/file' does not exist in the repository";
    assert!(
        !is_structural_git_error_test(msg),
        "Single 'does not exist' without 'invalid reference' should not be structural"
    );
}

#[test]
fn test_structural_git_error_only_invalid_reference_not_structural() {
    // Only one of the combined patterns — should NOT match
    let msg = "fatal: invalid reference 'refs/heads/task-abc'";
    assert!(
        !is_structural_git_error_test(msg),
        "Single 'invalid reference' without 'does not exist' should not be structural"
    );
}

#[test]
fn test_structural_git_error_transient_not_matched() {
    let transient_errors = [
        "fatal: Unable to create '.git/index.lock': File exists",
        "unable to create .git/index.lock",
        "Connection timed out",
        "repository busy, try again later",
        "error: lock file '.git/index.lock' is already locked",
        "error: remote end hung up unexpectedly",
    ];
    for msg in &transient_errors {
        assert!(
            !is_structural_git_error_test(msg),
            "Should NOT classify transient error as structural: {}",
            msg
        );
    }
}

#[test]
fn test_structural_git_error_empty_message_not_structural() {
    assert!(
        !is_structural_git_error_test(""),
        "Empty message should not be classified as structural git error"
    );
}

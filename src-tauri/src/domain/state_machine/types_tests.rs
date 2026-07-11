use super::*;

// ==================
// Blocker tests
// ==================

#[test]
fn test_blocker_new_creates_unresolved() {
    let blocker = Blocker::new("task-123");
    assert_eq!(blocker.id, "task-123");
    assert!(!blocker.resolved);
}

#[test]
fn test_blocker_human_input_creates_prefixed_id() {
    let blocker = Blocker::human_input("Need API credentials");
    assert!(blocker.id.starts_with("human:"));
    assert!(blocker.id.contains("Need API credentials"));
    assert!(!blocker.resolved);
}

#[test]
fn test_blocker_is_human_input_true_for_human_blockers() {
    let blocker = Blocker::human_input("Need approval");
    assert!(blocker.is_human_input());
}

#[test]
fn test_blocker_is_human_input_false_for_task_blockers() {
    let blocker = Blocker::new("task-456");
    assert!(!blocker.is_human_input());
}

#[test]
fn test_blocker_is_human_input_reason_handles_human_task_and_absent_reasons() {
    assert!(Blocker::is_human_input_reason(Some("human:Need approval")));
    assert!(!Blocker::is_human_input_reason(Some("task-456")));
    assert!(!Blocker::is_human_input_reason(None));
}

#[test]
fn test_blocker_is_freshness_blocked_reason_only_matches_the_terminal_sentinel() {
    assert!(Blocker::is_freshness_blocked_reason(Some(
        "FRESHNESS_BLOCKED|3|10|src/lib.rs|Persistent freshness conflicts"
    )));
    assert!(!Blocker::is_freshness_blocked_reason(Some(
        "human: approval"
    )));
    assert!(!Blocker::is_freshness_blocked_reason(None));
}

#[test]
fn test_blocker_resolve_sets_resolved_true() {
    let mut blocker = Blocker::new("task-789");
    assert!(!blocker.resolved);
    blocker.resolve();
    assert!(blocker.resolved);
}

#[test]
fn test_blocker_as_resolved_returns_new_resolved_blocker() {
    let blocker = Blocker::new("task-abc");
    assert!(!blocker.resolved);
    let resolved = blocker.as_resolved();
    assert!(resolved.resolved);
    assert_eq!(resolved.id, blocker.id);
    // Original unchanged
    assert!(!blocker.resolved);
}

#[test]
fn test_blocker_default_creates_empty() {
    let blocker = Blocker::default();
    assert_eq!(blocker.id, "");
    assert!(!blocker.resolved);
}

#[test]
fn test_blocker_clone_works() {
    let blocker = Blocker::new("task-clone");
    let cloned = blocker.clone();
    assert_eq!(blocker, cloned);
}

#[test]
fn test_blocker_equality_works() {
    let b1 = Blocker::new("task-1");
    let b2 = Blocker::new("task-1");
    let b3 = Blocker::new("task-2");
    assert_eq!(b1, b2);
    assert_ne!(b1, b3);
}

#[test]
fn test_blocker_equality_considers_resolved() {
    let b1 = Blocker::new("task-1");
    let b2 = b1.as_resolved();
    assert_ne!(b1, b2);
}

#[test]
fn test_blocker_serializes_to_json() {
    let blocker = Blocker::new("task-json");
    let json = serde_json::to_string(&blocker).unwrap();
    assert!(json.contains("task-json"));
    assert!(json.contains("resolved"));
}

#[test]
fn test_blocker_deserializes_from_json() {
    let json = r#"{"id":"task-parse","resolved":true}"#;
    let blocker: Blocker = serde_json::from_str(json).unwrap();
    assert_eq!(blocker.id, "task-parse");
    assert!(blocker.resolved);
}

#[test]
fn test_blocker_roundtrip_serialization() {
    let blockers = vec![
        Blocker::new("task-1"),
        Blocker::human_input("Need input"),
        Blocker::new("task-2").as_resolved(),
    ];

    for blocker in blockers {
        let json = serde_json::to_string(&blocker).unwrap();
        let restored: Blocker = serde_json::from_str(&json).unwrap();
        assert_eq!(blocker, restored);
    }
}

// ==================
// QaFailure tests
// ==================

#[test]
fn test_qa_failure_new_creates_with_name_and_error() {
    let failure = QaFailure::new("test_login", "Element not found");
    assert_eq!(failure.test_name, "test_login");
    assert_eq!(failure.error, "Element not found");
    assert!(failure.screenshot.is_none());
    assert!(failure.expected.is_none());
    assert!(failure.actual.is_none());
}

#[test]
fn test_qa_failure_assertion_failure_creates_with_expected_actual() {
    let failure = QaFailure::assertion_failure("test_count", "5", "3");
    assert_eq!(failure.test_name, "test_count");
    assert!(failure.error.contains("Expected '5'"));
    assert!(failure.error.contains("got '3'"));
    assert_eq!(failure.expected, Some("5".to_string()));
    assert_eq!(failure.actual, Some("3".to_string()));
}

#[test]
fn test_qa_failure_visual_failure_creates_with_screenshot() {
    let failure = QaFailure::visual_failure(
        "test_button_visible",
        "Button not visible",
        "screenshots/button_test.png",
    );
    assert_eq!(failure.test_name, "test_button_visible");
    assert_eq!(failure.error, "Button not visible");
    assert_eq!(
        failure.screenshot,
        Some("screenshots/button_test.png".to_string())
    );
}

#[test]
fn test_qa_failure_with_screenshot_adds_path() {
    let failure =
        QaFailure::new("test_render", "Render failed").with_screenshot("screenshots/render.png");
    assert_eq!(
        failure.screenshot,
        Some("screenshots/render.png".to_string())
    );
}

#[test]
fn test_qa_failure_default_creates_empty() {
    let failure = QaFailure::default();
    assert_eq!(failure.test_name, "");
    assert_eq!(failure.error, "");
    assert!(failure.screenshot.is_none());
    assert!(failure.expected.is_none());
    assert!(failure.actual.is_none());
}

#[test]
fn test_qa_failure_clone_works() {
    let failure = QaFailure::new("test_clone", "Clone error");
    let cloned = failure.clone();
    assert_eq!(failure, cloned);
}

#[test]
fn test_qa_failure_equality_works() {
    let f1 = QaFailure::new("test_1", "Error 1");
    let f2 = QaFailure::new("test_1", "Error 1");
    let f3 = QaFailure::new("test_2", "Error 2");
    assert_eq!(f1, f2);
    assert_ne!(f1, f3);
}

#[test]
fn test_qa_failure_serializes_to_json() {
    let failure = QaFailure::new("test_json", "JSON error").with_screenshot("screen.png");
    let json = serde_json::to_string(&failure).unwrap();
    assert!(json.contains("test_json"));
    assert!(json.contains("JSON error"));
    assert!(json.contains("screen.png"));
}

#[test]
fn test_qa_failure_deserializes_from_json() {
    let json = r#"{
        "test_name": "test_parse",
        "error": "Parse error",
        "screenshot": null,
        "expected": "foo",
        "actual": "bar"
    }"#;
    let failure: QaFailure = serde_json::from_str(json).unwrap();
    assert_eq!(failure.test_name, "test_parse");
    assert_eq!(failure.error, "Parse error");
    assert!(failure.screenshot.is_none());
    assert_eq!(failure.expected, Some("foo".to_string()));
    assert_eq!(failure.actual, Some("bar".to_string()));
}

#[test]
fn test_qa_failure_roundtrip_serialization() {
    let failures = vec![
        QaFailure::new("test_1", "Error 1"),
        QaFailure::assertion_failure("test_2", "a", "b"),
        QaFailure::visual_failure("test_3", "Visual fail", "screen.png"),
        QaFailure::new("test_4", "Error 4").with_screenshot("img.png"),
    ];

    for failure in failures {
        let json = serde_json::to_string(&failure).unwrap();
        let restored: QaFailure = serde_json::from_str(&json).unwrap();
        assert_eq!(failure, restored);
    }
}

#[test]
fn test_qa_failure_debug_format() {
    let failure = QaFailure::new("test_debug", "Debug error");
    let debug_str = format!("{:?}", failure);
    assert!(debug_str.contains("QaFailure"));
    assert!(debug_str.contains("test_debug"));
}

// ==================
// QaFailedData tests
// ==================

#[test]
fn test_qa_failed_data_new_creates_with_failures() {
    let failures = vec![
        QaFailure::new("test_1", "Error 1"),
        QaFailure::new("test_2", "Error 2"),
    ];
    let data = QaFailedData::new(failures.clone());
    assert_eq!(data.failures.len(), 2);
    assert_eq!(data.retry_count, 0);
    assert!(!data.notified);
}

#[test]
fn test_qa_failed_data_single_creates_from_one_failure() {
    let data = QaFailedData::single(QaFailure::new("test_single", "Single error"));
    assert_eq!(data.failures.len(), 1);
    assert_eq!(data.failures[0].test_name, "test_single");
}

#[test]
fn test_qa_failed_data_default_creates_empty() {
    let data = QaFailedData::default();
    assert!(data.failures.is_empty());
    assert_eq!(data.retry_count, 0);
    assert!(!data.notified);
}

#[test]
fn test_qa_failed_data_has_failures() {
    let empty = QaFailedData::default();
    assert!(!empty.has_failures());

    let with_failure = QaFailedData::single(QaFailure::new("test", "error"));
    assert!(with_failure.has_failures());
}

#[test]
fn test_qa_failed_data_failure_count() {
    let data = QaFailedData::new(vec![
        QaFailure::new("t1", "e1"),
        QaFailure::new("t2", "e2"),
        QaFailure::new("t3", "e3"),
    ]);
    assert_eq!(data.failure_count(), 3);
}

#[test]
fn test_qa_failed_data_add_failure() {
    let mut data = QaFailedData::default();
    assert_eq!(data.failure_count(), 0);
    data.add_failure(QaFailure::new("test", "error"));
    assert_eq!(data.failure_count(), 1);
}

#[test]
fn test_qa_failed_data_increment_retry() {
    let mut data = QaFailedData::default();
    assert_eq!(data.retry_count, 0);
    data.increment_retry();
    assert_eq!(data.retry_count, 1);
    data.increment_retry();
    assert_eq!(data.retry_count, 2);
}

#[test]
fn test_qa_failed_data_mark_notified() {
    let mut data = QaFailedData::default();
    assert!(!data.notified);
    data.mark_notified();
    assert!(data.notified);
}

#[test]
fn test_qa_failed_data_first_error() {
    let empty = QaFailedData::default();
    assert!(empty.first_error().is_none());

    let data = QaFailedData::new(vec![
        QaFailure::new("test_1", "First error"),
        QaFailure::new("test_2", "Second error"),
    ]);
    assert_eq!(data.first_error(), Some("First error"));
}

#[test]
fn test_qa_failed_data_serializes_to_json() {
    let data = QaFailedData::single(QaFailure::new("test_json", "JSON error"));
    let json = serde_json::to_string(&data).unwrap();
    assert!(json.contains("test_json"));
    assert!(json.contains("retry_count"));
}

#[test]
fn test_qa_failed_data_deserializes_from_json() {
    let json = r#"{
        "failures": [{"test_name": "t1", "error": "e1", "screenshot": null, "expected": null, "actual": null}],
        "retry_count": 3,
        "notified": true
    }"#;
    let data: QaFailedData = serde_json::from_str(json).unwrap();
    assert_eq!(data.failures.len(), 1);
    assert_eq!(data.retry_count, 3);
    assert!(data.notified);
}

#[test]
fn test_qa_failed_data_roundtrip_serialization() {
    let data = QaFailedData::new(vec![
        QaFailure::new("test_1", "Error 1"),
        QaFailure::assertion_failure("test_2", "a", "b"),
    ]);
    let json = serde_json::to_string(&data).unwrap();
    let restored: QaFailedData = serde_json::from_str(&json).unwrap();
    assert_eq!(data, restored);
}

#[test]
fn test_qa_failed_data_clone_works() {
    let data = QaFailedData::single(QaFailure::new("test", "error"));
    let cloned = data.clone();
    assert_eq!(data, cloned);
}

// ==================
// FailedData tests
// ==================

#[test]
fn test_failed_data_new_creates_with_error() {
    let data = FailedData::new("Build failed");
    assert_eq!(data.error, "Build failed");
    assert!(data.details.is_none());
    assert!(!data.is_timeout);
    assert!(!data.notified);
}

#[test]
fn test_failed_data_timeout_creates_timeout_failure() {
    let data = FailedData::timeout("Operation timed out after 60s");
    assert_eq!(data.error, "Operation timed out after 60s");
    assert!(data.is_timeout);
}

#[test]
fn test_failed_data_default_creates_empty() {
    let data = FailedData::default();
    assert_eq!(data.error, "");
    assert!(data.details.is_none());
    assert!(!data.is_timeout);
    assert!(!data.notified);
}

#[test]
fn test_failed_data_with_details_adds_details() {
    let data =
        FailedData::new("Compilation error").with_details("error[E0382]: borrow of moved value");
    assert_eq!(data.error, "Compilation error");
    assert_eq!(
        data.details,
        Some("error[E0382]: borrow of moved value".to_string())
    );
}

#[test]
fn test_failed_data_mark_notified() {
    let mut data = FailedData::new("Error");
    assert!(!data.notified);
    data.mark_notified();
    assert!(data.notified);
}

#[test]
fn test_failed_data_serializes_to_json() {
    let data = FailedData::new("Test error").with_details("Stack trace...");
    let json = serde_json::to_string(&data).unwrap();
    assert!(json.contains("Test error"));
    assert!(json.contains("Stack trace"));
}

#[test]
fn test_failed_data_deserializes_from_json() {
    let json = r#"{
        "error": "Parse error",
        "details": "Line 42",
        "is_timeout": false,
        "notified": true
    }"#;
    let data: FailedData = serde_json::from_str(json).unwrap();
    assert_eq!(data.error, "Parse error");
    assert_eq!(data.details, Some("Line 42".to_string()));
    assert!(!data.is_timeout);
    assert!(data.notified);
}

#[test]
fn test_failed_data_roundtrip_serialization() {
    let cases = vec![
        FailedData::new("Error 1"),
        FailedData::timeout("Timeout error"),
        FailedData::new("Error 2").with_details("Details here"),
    ];

    for data in cases {
        let json = serde_json::to_string(&data).unwrap();
        let restored: FailedData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, restored);
    }
}

#[test]
fn test_failed_data_clone_works() {
    let data = FailedData::new("Clone error");
    let cloned = data.clone();
    assert_eq!(data, cloned);
}

#[test]
fn test_failed_data_equality_works() {
    let d1 = FailedData::new("Error");
    let d2 = FailedData::new("Error");
    let d3 = FailedData::new("Different");
    assert_eq!(d1, d2);
    assert_ne!(d1, d3);
}

#[test]
fn test_failed_data_attempt_count_defaults_to_zero() {
    let data = FailedData::new("Build failed");
    assert_eq!(data.attempt_count, 0);
}

#[test]
fn test_failed_data_with_attempt_count_sets_count() {
    let data = FailedData::new("Build failed").with_attempt_count(3);
    assert_eq!(data.attempt_count, 3);
}

#[test]
fn test_failed_data_default_attempt_count_is_zero() {
    let data = FailedData::default();
    assert_eq!(data.attempt_count, 0);
}

#[test]
fn test_failed_data_serde_default_when_field_missing() {
    // Legacy JSON without attempt_count field should deserialize to 0
    let json = r#"{"error":"Parse error","details":null,"is_timeout":false,"notified":false}"#;
    let data: FailedData = serde_json::from_str(json).unwrap();
    assert_eq!(data.attempt_count, 0, "Missing field should default to 0");
}

#[test]
fn test_failed_data_serde_roundtrip_with_attempt_count() {
    let data = FailedData::new("Timeout").with_attempt_count(5);
    let json = serde_json::to_string(&data).unwrap();
    let restored: FailedData = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.attempt_count, 5);
    assert_eq!(restored.error, "Timeout");
}

#[test]
fn test_failed_data_attempt_count_populates_from_metadata() {
    // Simulate the on_enter(Failed) handler reading auto_retry_count_executing
    let task_metadata = r#"{"auto_retry_count_executing": 3, "other_key": "value"}"#;
    let attempt_count = serde_json::from_str::<serde_json::Value>(task_metadata)
        .ok()
        .and_then(|v| v.get("auto_retry_count_executing").and_then(|c| c.as_u64()))
        .unwrap_or(0) as u32;
    let data = FailedData::new("Error").with_attempt_count(attempt_count);
    assert_eq!(data.attempt_count, 3);
}

#[test]
fn test_failed_data_attempt_count_zero_when_metadata_key_absent() {
    // Task metadata exists but has no auto_retry_count_executing key
    let task_metadata = r#"{"trigger_origin": "scheduler"}"#;
    let attempt_count = serde_json::from_str::<serde_json::Value>(task_metadata)
        .ok()
        .and_then(|v| v.get("auto_retry_count_executing").and_then(|c| c.as_u64()))
        .unwrap_or(0) as u32;
    assert_eq!(attempt_count, 0);
    let data = FailedData::new("Error").with_attempt_count(attempt_count);
    assert_eq!(data.attempt_count, 0);
}

#[test]
fn test_failed_data_attempt_count_zero_when_no_metadata() {
    // No metadata at all
    let attempt_count = (None::<&str>)
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("auto_retry_count_executing").and_then(|c| c.as_u64()))
        .unwrap_or(0) as u32;
    assert_eq!(attempt_count, 0);
}

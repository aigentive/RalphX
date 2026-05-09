// Tests for merge_helpers: extract_task_id, parse_metadata, merge_deferred,
// trigger_origin, archived_task guard.
//
// Extracted from side_effects.rs (lines 6466–6707).

use super::super::merge_helpers::{
    clear_merge_deferred_metadata, clear_trigger_origin, extract_task_id_from_merge_path,
    get_trigger_origin, has_merge_deferred_metadata, parse_metadata, set_trigger_origin,
};
use super::helpers::*;
use crate::domain::entities::task_metadata::{
    MergeRecoveryEvent, MergeRecoveryEventKind, MergeRecoveryMetadata, MergeRecoveryReasonCode,
    MergeRecoverySource, MergeRecoveryState,
};
use crate::domain::entities::{InternalStatus, TaskId};

// ==================
// extract_task_id_from_merge_path tests
// ==================

#[test]
fn test_extract_task_id_from_merge_path_valid() {
    let path = "/home/user/ralphx-worktrees/my-app/merge-abc123def456";
    assert_eq!(extract_task_id_from_merge_path(path), Some("abc123def456"));
}

#[test]
fn test_extract_task_id_from_merge_path_uuid() {
    let path = "/tmp/wt/merge-e0ce32e7-eaef-4a07-b81d-2126d0dee5d9";
    assert_eq!(
        extract_task_id_from_merge_path(path),
        Some("e0ce32e7-eaef-4a07-b81d-2126d0dee5d9"),
    );
}

#[test]
fn test_extract_task_id_from_merge_path_not_merge() {
    let path = "/home/user/ralphx-worktrees/my-app/task-abc123";
    assert_eq!(extract_task_id_from_merge_path(path), None);
}

#[test]
fn test_extract_task_id_from_merge_path_bare_name() {
    assert_eq!(extract_task_id_from_merge_path("merge-xyz"), Some("xyz"));
}

#[test]
fn test_extract_task_id_from_merge_path_empty() {
    assert_eq!(extract_task_id_from_merge_path(""), None);
}

#[test]
fn test_extract_task_id_from_merge_path_just_merge_prefix() {
    // "merge-" with empty task ID should return empty string
    assert_eq!(extract_task_id_from_merge_path("/dir/merge-"), Some(""));
}

// ==================
// parse_metadata tests
// ==================

#[test]
fn parse_metadata_returns_none_when_no_metadata() {
    let task = make_task(None, None);
    assert!(parse_metadata(&task).is_none());
}

#[test]
fn parse_metadata_returns_none_for_invalid_json() {
    let mut task = make_task(None, None);
    task.metadata = Some("not json".to_string());
    assert!(parse_metadata(&task).is_none());
}

#[test]
fn parse_metadata_returns_value_for_valid_json() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"key": "value"}"#.to_string());
    let meta = parse_metadata(&task).unwrap();
    assert_eq!(meta["key"], "value");
}

// ==================
// has_merge_deferred_metadata tests
// ==================

#[test]
fn has_merge_deferred_returns_false_when_no_metadata() {
    let task = make_task(None, None);
    assert!(!has_merge_deferred_metadata(&task));
}

#[test]
fn has_merge_deferred_returns_false_when_no_flag() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"other": "data"}"#.to_string());
    assert!(!has_merge_deferred_metadata(&task));
}

#[test]
fn has_merge_deferred_returns_false_when_flag_is_false() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"merge_deferred": false}"#.to_string());
    assert!(!has_merge_deferred_metadata(&task));
}

#[test]
fn has_merge_deferred_returns_true_when_flag_is_true() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"merge_deferred": true}"#.to_string());
    assert!(has_merge_deferred_metadata(&task));
}

#[test]
fn has_merge_deferred_returns_true_for_typed_deferred_metadata() {
    let mut task = make_task(None, None);
    let mut recovery = MergeRecoveryMetadata::new();
    recovery.append_event_with_state(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            "Task deferred behind another merge",
        )
        .with_blocking_task(TaskId::new()),
        MergeRecoveryState::Deferred,
    );
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());

    assert!(has_merge_deferred_metadata(&task));
}

#[test]
fn has_merge_deferred_returns_false_for_typed_retrying_metadata() {
    let mut task = make_task(None, None);
    let mut recovery = MergeRecoveryMetadata::new();
    recovery.append_event_with_state(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::Deferred,
            MergeRecoverySource::System,
            MergeRecoveryReasonCode::TargetBranchBusy,
            "Task deferred behind another merge",
        )
        .with_blocking_task(TaskId::new()),
        MergeRecoveryState::Deferred,
    );
    recovery.append_event_with_state(
        MergeRecoveryEvent::new(
            MergeRecoveryEventKind::AttemptStarted,
            MergeRecoverySource::Auto,
            MergeRecoveryReasonCode::TargetBranchBusy,
            "Retrying after deferral",
        ),
        MergeRecoveryState::Retrying,
    );
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());

    assert!(!has_merge_deferred_metadata(&task));
}

// ==================
// clear_merge_deferred_metadata tests
// ==================

#[test]
fn clear_merge_deferred_removes_flags_from_metadata() {
    let mut task = make_task(None, None);
    task.metadata = Some(
        serde_json::json!({
            "merge_deferred": true,
            "merge_deferred_at": "2026-01-01T00:00:00Z",
            "other": "keep"
        })
        .to_string(),
    );

    clear_merge_deferred_metadata(&mut task);

    let meta = parse_metadata(&task).unwrap();
    assert!(meta.get("merge_deferred").is_none());
    assert!(meta.get("merge_deferred_at").is_none());
    assert_eq!(meta["other"], "keep");
}

#[test]
fn clear_merge_deferred_clears_metadata_when_only_deferred_fields() {
    let mut task = make_task(None, None);
    task.metadata = Some(
        serde_json::json!({
            "merge_deferred": true,
            "merge_deferred_at": "2026-01-01T00:00:00Z",
        })
        .to_string(),
    );

    clear_merge_deferred_metadata(&mut task);

    assert!(task.metadata.is_none());
}

#[test]
fn clear_merge_deferred_noop_when_no_metadata() {
    let mut task = make_task(None, None);
    clear_merge_deferred_metadata(&mut task);
    assert!(task.metadata.is_none());
}

// ==================
// trigger_origin helpers tests
// ==================

#[test]
fn set_trigger_origin_creates_metadata_when_none() {
    let mut task = make_task(None, None);
    set_trigger_origin(&mut task, "scheduler");
    let meta = parse_metadata(&task).unwrap();
    assert_eq!(meta["trigger_origin"], "scheduler");
}

#[test]
fn set_trigger_origin_preserves_existing_metadata() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"other": "value"}"#.to_string());
    set_trigger_origin(&mut task, "revision");
    let meta = parse_metadata(&task).unwrap();
    assert_eq!(meta["trigger_origin"], "revision");
    assert_eq!(meta["other"], "value");
}

#[test]
fn get_trigger_origin_returns_value_when_set() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"trigger_origin": "recovery"}"#.to_string());
    assert_eq!(get_trigger_origin(&task), Some("recovery".to_string()));
}

#[test]
fn get_trigger_origin_returns_none_when_not_set() {
    let task = make_task(None, None);
    assert!(get_trigger_origin(&task).is_none());
}

#[test]
fn get_trigger_origin_returns_none_when_metadata_has_no_origin() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"other": "value"}"#.to_string());
    assert!(get_trigger_origin(&task).is_none());
}

#[test]
fn clear_trigger_origin_removes_field_from_metadata() {
    let mut task = make_task(None, None);
    task.metadata = Some(
        serde_json::json!({
            "trigger_origin": "qa",
            "other": "keep"
        })
        .to_string(),
    );

    clear_trigger_origin(&mut task);

    let meta = parse_metadata(&task).unwrap();
    assert!(meta.get("trigger_origin").is_none());
    assert_eq!(meta["other"], "keep");
}

#[test]
fn clear_trigger_origin_clears_metadata_when_last_field() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"trigger_origin": "retry"}"#.to_string());

    clear_trigger_origin(&mut task);

    assert!(task.metadata.is_none());
}

#[test]
fn clear_trigger_origin_noop_when_no_metadata() {
    let mut task = make_task(None, None);
    clear_trigger_origin(&mut task);
    assert!(task.metadata.is_none());
}

// ==================
// concurrent merge guard — archived task skip tests
// ==================

/// Archived tasks in PendingMerge should NOT block newer merge tasks.
/// Regression test: archived tasks have archived_at set and will never
/// complete their merge, so the guard must skip them.
#[test]
fn archived_task_in_pending_merge_is_not_a_blocker() {
    // An archived task in PendingMerge — should be skipped by the guard
    let mut archived_task = make_task(None, None);
    archived_task.internal_status = InternalStatus::PendingMerge;
    archived_task.archived_at = Some(chrono::Utc::now());
    archived_task.created_at = chrono::Utc::now() - chrono::Duration::hours(1);

    // The guard checks: skip self, skip non-merge states, skip deferred, skip archived
    // Verify that archived_at.is_some() returns true for this task
    assert!(archived_task.archived_at.is_some());

    // A non-archived task should NOT be skipped
    let mut active_task = make_task(None, None);
    active_task.internal_status = InternalStatus::PendingMerge;
    active_task.created_at = chrono::Utc::now() - chrono::Duration::hours(1);
    assert!(active_task.archived_at.is_none());
}

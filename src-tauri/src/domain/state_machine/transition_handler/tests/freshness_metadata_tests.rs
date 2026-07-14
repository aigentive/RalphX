// Unit tests for FreshnessMetadata struct.
//
// Tests:
//   1. from_task_metadata: deserializes all fields correctly
//   2. from_task_metadata: returns defaults for absent/empty/non-object metadata
//   3. merge_into: writes all fields back preserving non-freshness keys
//   4. clear_from: removes all freshness keys, preserves others
//   5. Serde round-trip: struct → merge_into → from_task_metadata roundtrips correctly

use crate::domain::state_machine::transition_handler::freshness::FreshnessMetadata;
use serde_json::{json, Value};

// ==================
// from_task_metadata
// ==================

#[test]
fn test_from_metadata_deserializes_all_fields() {
    let metadata = json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "executing",
        "freshness_conflict_count": 2,
        "plan_update_conflict": true,
        "source_update_conflict": false,
        "last_freshness_check_at": "2026-03-08T02:00:00Z",
        "conflict_files": ["src/foo.rs", "src/bar.rs"],
        "source_branch": "task/abc",
        "target_branch": "plan/xyz"
    });

    let fm = FreshnessMetadata::from_task_metadata(&metadata);

    assert!(fm.branch_freshness_conflict);
    assert_eq!(fm.freshness_origin_state.as_deref(), Some("executing"));
    assert_eq!(fm.freshness_conflict_count, 2);
    assert!(fm.plan_update_conflict);
    assert!(!fm.source_update_conflict);
    assert_eq!(
        fm.last_freshness_check_at.as_deref(),
        Some("2026-03-08T02:00:00Z")
    );
    assert_eq!(fm.conflict_files, vec!["src/foo.rs", "src/bar.rs"]);
    assert_eq!(fm.source_branch.as_deref(), Some("task/abc"));
    assert_eq!(fm.target_branch.as_deref(), Some("plan/xyz"));
}

#[test]
fn test_from_metadata_returns_defaults_for_empty_object() {
    let metadata = json!({});
    let fm = FreshnessMetadata::from_task_metadata(&metadata);

    assert!(!fm.branch_freshness_conflict);
    assert!(fm.freshness_origin_state.is_none());
    assert_eq!(fm.freshness_conflict_count, 0);
    assert!(!fm.plan_update_conflict);
    assert!(!fm.source_update_conflict);
    assert!(fm.last_freshness_check_at.is_none());
    assert!(fm.conflict_files.is_empty());
    assert!(fm.source_branch.is_none());
    assert!(fm.target_branch.is_none());
}

#[test]
fn test_from_metadata_returns_defaults_for_null_metadata() {
    let metadata = Value::Null;
    let fm = FreshnessMetadata::from_task_metadata(&metadata);
    assert_eq!(fm, FreshnessMetadata::default());
}

#[test]
fn test_from_metadata_returns_defaults_for_non_object() {
    let metadata = json!("not an object");
    let fm = FreshnessMetadata::from_task_metadata(&metadata);
    assert_eq!(fm, FreshnessMetadata::default());
}

#[test]
fn test_from_metadata_handles_partial_fields() {
    // Only some fields present — others default
    let metadata = json!({
        "branch_freshness_conflict": true,
        "freshness_conflict_count": 1
    });
    let fm = FreshnessMetadata::from_task_metadata(&metadata);

    assert!(fm.branch_freshness_conflict);
    assert_eq!(fm.freshness_conflict_count, 1);
    assert!(fm.freshness_origin_state.is_none());
    assert!(fm.conflict_files.is_empty());
}

// ==================
// merge_into
// ==================

#[test]
fn test_merge_into_writes_all_fields() {
    let fm = FreshnessMetadata {
        branch_freshness_conflict: true,
        freshness_origin_state: Some("re_executing".to_owned()),
        freshness_conflict_count: 1,
        plan_update_conflict: true,
        source_update_conflict: false,
        last_freshness_check_at: Some("2026-03-08T03:00:00Z".to_owned()),
        last_plan_freshness_check_at: Some("2026-03-08T03:00:00Z".to_owned()),
        last_task_freshness_check_at: Some("2026-03-08T03:00:00Z".to_owned()),
        conflict_files: vec!["src/main.rs".to_owned()],
        source_branch: Some("task/branch".to_owned()),
        target_branch: Some("plan/branch".to_owned()),
        freshness_backoff_until: None,
        freshness_auto_reset_count: 0,
        freshness_count_incremented_by: None,
    };

    let mut metadata = json!({});
    fm.merge_into(&mut metadata);

    assert_eq!(metadata["branch_freshness_conflict"], true);
    assert_eq!(metadata["freshness_origin_state"], "re_executing");
    assert_eq!(metadata["freshness_conflict_count"], 1);
    assert_eq!(metadata["plan_update_conflict"], true);
    assert_eq!(metadata["source_update_conflict"], false);
    assert_eq!(metadata["last_freshness_check_at"], "2026-03-08T03:00:00Z");
    assert_eq!(metadata["conflict_files"], json!(["src/main.rs"]));
    assert_eq!(metadata["source_branch"], "task/branch");
    assert_eq!(metadata["target_branch"], "plan/branch");
}

#[test]
fn test_merge_into_preserves_existing_non_freshness_keys() {
    let fm = FreshnessMetadata {
        branch_freshness_conflict: true,
        ..Default::default()
    };

    let mut metadata = json!({
        "trigger_origin": "scheduler",
        "some_other_field": 42
    });
    fm.merge_into(&mut metadata);

    // Existing keys preserved
    assert_eq!(metadata["trigger_origin"], "scheduler");
    assert_eq!(metadata["some_other_field"], 42);
    // Freshness field written
    assert_eq!(metadata["branch_freshness_conflict"], true);
}

#[test]
fn test_merge_into_removes_optional_fields_when_none() {
    // Start with some optional fields set, then merge a struct with None values
    let fm = FreshnessMetadata {
        freshness_origin_state: None,
        last_freshness_check_at: None,
        source_branch: None,
        target_branch: None,
        ..Default::default()
    };

    let mut metadata = json!({
        "freshness_origin_state": "executing",
        "last_freshness_check_at": "2026-03-08T00:00:00Z",
        "source_branch": "task/old",
        "target_branch": "plan/old"
    });
    fm.merge_into(&mut metadata);

    // Optional fields with None values should be removed
    assert!(
        metadata.get("freshness_origin_state").is_none()
            || metadata["freshness_origin_state"].is_null(),
        "None optional field should be removed from metadata"
    );
}

#[test]
fn test_merge_into_is_noop_for_non_object() {
    let fm = FreshnessMetadata {
        branch_freshness_conflict: true,
        ..Default::default()
    };

    let mut metadata = Value::Null;
    // Should not panic
    fm.merge_into(&mut metadata);
    // Metadata unchanged (still Null)
    assert!(metadata.is_null());
}

// ==================
// clear_from
// ==================

#[test]
fn test_clear_from_removes_all_freshness_keys() {
    let mut metadata = json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "executing",
        "freshness_conflict_count": 2,
        "plan_update_conflict": true,
        "source_update_conflict": false,
        "last_freshness_check_at": "2026-03-08T02:00:00Z",
        "conflict_files": ["src/foo.rs"],
        "source_branch": "task/abc",
        "target_branch": "plan/xyz"
    });

    FreshnessMetadata::clear_from(&mut metadata);

    let obj = metadata.as_object().unwrap();
    assert!(!obj.contains_key("branch_freshness_conflict"));
    assert!(!obj.contains_key("freshness_origin_state"));
    assert!(!obj.contains_key("freshness_conflict_count"));
    assert!(!obj.contains_key("plan_update_conflict"));
    assert!(!obj.contains_key("source_update_conflict"));
    assert!(!obj.contains_key("last_freshness_check_at"));
    assert!(!obj.contains_key("conflict_files"));
    assert!(!obj.contains_key("source_branch"));
    assert!(!obj.contains_key("target_branch"));
}

#[test]
fn test_clear_from_preserves_non_freshness_keys() {
    let mut metadata = json!({
        "branch_freshness_conflict": true,
        "trigger_origin": "scheduler",
        "merge_commit_sha": "abc123"
    });

    FreshnessMetadata::clear_from(&mut metadata);

    let obj = metadata.as_object().unwrap();
    assert!(!obj.contains_key("branch_freshness_conflict"));
    assert_eq!(metadata["trigger_origin"], "scheduler");
    assert_eq!(metadata["merge_commit_sha"], "abc123");
}

#[test]
fn test_clear_from_empty_object_is_noop() {
    let mut metadata = json!({});
    // Should not panic
    FreshnessMetadata::clear_from(&mut metadata);
    assert_eq!(metadata.as_object().unwrap().len(), 0);
}

#[test]
fn test_clear_from_non_object_is_noop() {
    let mut metadata = Value::Null;
    // Should not panic
    FreshnessMetadata::clear_from(&mut metadata);
    assert!(metadata.is_null());
}

// ==================
// Round-trip: merge_into then from_task_metadata
// ==================

#[test]
fn test_serde_round_trip_full_struct() {
    let original = FreshnessMetadata {
        branch_freshness_conflict: true,
        freshness_origin_state: Some("reviewing".to_owned()),
        freshness_conflict_count: 3,
        plan_update_conflict: false,
        source_update_conflict: true,
        last_freshness_check_at: Some("2026-03-08T04:00:00Z".to_owned()),
        last_plan_freshness_check_at: Some("2026-03-08T04:00:00Z".to_owned()),
        last_task_freshness_check_at: Some("2026-03-08T04:00:00Z".to_owned()),
        conflict_files: vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()],
        source_branch: Some("task/t1".to_owned()),
        target_branch: Some("plan/p1".to_owned()),
        freshness_backoff_until: None,
        freshness_auto_reset_count: 0,
        freshness_count_incremented_by: None,
    };

    let mut metadata = json!({});
    original.merge_into(&mut metadata);

    let recovered = FreshnessMetadata::from_task_metadata(&metadata);
    assert_eq!(
        recovered, original,
        "Round-trip must produce identical struct"
    );
}

#[test]
fn test_serde_round_trip_defaults() {
    let original = FreshnessMetadata::default();

    let mut metadata = json!({});
    original.merge_into(&mut metadata);

    let recovered = FreshnessMetadata::from_task_metadata(&metadata);
    assert_eq!(
        recovered, original,
        "Default struct must round-trip correctly"
    );
}

#[test]
fn test_serde_round_trip_with_existing_keys() {
    let original = FreshnessMetadata {
        branch_freshness_conflict: true,
        freshness_origin_state: Some("executing".to_owned()),
        freshness_conflict_count: 1,
        ..Default::default()
    };

    // Start with other metadata present
    let mut metadata = json!({ "trigger_origin": "scheduler" });
    original.merge_into(&mut metadata);

    let recovered = FreshnessMetadata::from_task_metadata(&metadata);
    assert_eq!(recovered, original);

    // Confirm other key still present
    assert_eq!(metadata["trigger_origin"], "scheduler");
}

// ==================
// clear_from after merge_into leaves clean state
// ==================

#[test]
fn test_clear_from_after_merge_into_leaves_only_other_keys() {
    let fm = FreshnessMetadata {
        branch_freshness_conflict: true,
        freshness_origin_state: Some("executing".to_owned()),
        freshness_conflict_count: 1,
        ..Default::default()
    };

    let mut metadata = json!({ "trigger_origin": "scheduler" });
    fm.merge_into(&mut metadata);

    // Freshness keys present
    assert_eq!(metadata["branch_freshness_conflict"], true);

    FreshnessMetadata::clear_from(&mut metadata);

    // After clear, freshness keys gone
    let obj = metadata.as_object().unwrap();
    assert!(!obj.contains_key("branch_freshness_conflict"));
    assert!(!obj.contains_key("freshness_origin_state"));
    assert!(!obj.contains_key("freshness_conflict_count"));
    // Other key preserved
    assert_eq!(metadata["trigger_origin"], "scheduler");

    // from_task_metadata now returns defaults
    let recovered = FreshnessMetadata::from_task_metadata(&metadata);
    assert_eq!(recovered, FreshnessMetadata::default());
}

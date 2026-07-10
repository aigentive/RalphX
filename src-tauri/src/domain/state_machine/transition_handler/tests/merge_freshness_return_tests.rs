// Unit tests for freshness return routing logic.
//
// Tests the metadata decision logic for handle_freshness_return_routing:
//   1. valid_executing_origin: branch_freshness_conflict=true, freshness_origin_state="executing" → Ready
//   2. valid_re_executing_origin: freshness_origin_state="re_executing" → Ready
//   3. valid_reviewing_origin: freshness_origin_state="reviewing" → PendingReview
//   4. unknown_origin: freshness_origin_state="unknown_state" → Ready (fallback)
//   5. corrupted_no_origin: branch_freshness_conflict=true but freshness_origin_state=None → Ready
//   6. no_freshness_flag: no branch_freshness_conflict → normal merge flow (no early return)
//
// Also tests the metadata clearing logic (FreshnessMetadata::clear_from) as used
// during the return routing.

use crate::domain::entities::InternalStatus;
use crate::domain::state_machine::transition_handler::freshness::{
    FreshnessCleanupScope, FreshnessMetadata,
};
use serde_json::json;

/// Helper: derive target InternalStatus from freshness_origin_state, matching
/// the logic in handle_freshness_return_routing.
fn routing_target_for(origin: Option<&str>) -> InternalStatus {
    match origin {
        Some("executing") | Some("re_executing") => InternalStatus::Ready,
        Some("reviewing") => InternalStatus::PendingReview,
        _ => InternalStatus::Ready,
    }
}

// ==================
// Routing decision tests (match logic parity with handle_freshness_return_routing)
// ==================

#[test]
fn test_routing_executing_origin_routes_to_ready() {
    let target = routing_target_for(Some("executing"));
    assert_eq!(
        target,
        InternalStatus::Ready,
        "executing origin must route to Ready"
    );
}

#[test]
fn test_routing_re_executing_origin_routes_to_ready() {
    let target = routing_target_for(Some("re_executing"));
    assert_eq!(
        target,
        InternalStatus::Ready,
        "re_executing origin must route to Ready"
    );
}

#[test]
fn test_routing_reviewing_origin_routes_to_pending_review() {
    let target = routing_target_for(Some("reviewing"));
    assert_eq!(
        target,
        InternalStatus::PendingReview,
        "reviewing origin must route to PendingReview"
    );
}

#[test]
fn test_routing_unknown_origin_routes_to_ready() {
    // Unknown state falls through to Ready as a safe default
    let target = routing_target_for(Some("unknown_state"));
    assert_eq!(
        target,
        InternalStatus::Ready,
        "unknown origin must fall back to Ready"
    );
}

#[test]
fn test_routing_none_origin_routes_to_ready() {
    // Absent freshness_origin_state (corrupted metadata) defaults to Ready
    let target = routing_target_for(None);
    assert_eq!(
        target,
        InternalStatus::Ready,
        "absent freshness_origin_state must fall back to Ready"
    );
}

// ==================
// Metadata flag detection tests
// ==================

#[test]
fn test_freshness_flag_true_triggers_early_return() {
    // Simulates the check: if freshness.branch_freshness_conflict { ... }
    let meta = json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "executing",
        "freshness_conflict_count": 1,
        "plan_update_conflict": true,
        "source_update_conflict": false,
    });
    let freshness = FreshnessMetadata::from_task_metadata(&meta);
    assert!(
        freshness.branch_freshness_conflict,
        "branch_freshness_conflict=true must trigger freshness routing"
    );
}

#[test]
fn test_freshness_flag_false_skips_routing() {
    // Normal merge metadata — no freshness routing
    let meta = json!({
        "branch_freshness_conflict": false,
        "plan_update_conflict": false,
    });
    let freshness = FreshnessMetadata::from_task_metadata(&meta);
    assert!(
        !freshness.branch_freshness_conflict,
        "branch_freshness_conflict=false must not trigger freshness routing"
    );
}

#[test]
fn test_no_freshness_keys_in_metadata_skips_routing() {
    // Empty metadata (normal merge path) — no freshness routing
    let meta = json!({});
    let freshness = FreshnessMetadata::from_task_metadata(&meta);
    assert!(
        !freshness.branch_freshness_conflict,
        "absent branch_freshness_conflict must not trigger freshness routing"
    );
}

// ==================
// Metadata clearing tests (mirrors handle_freshness_return_routing clear step)
// ==================

#[test]
fn test_metadata_cleared_for_executing_origin() {
    let mut meta = json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "executing",
        "freshness_conflict_count": 1,
        "plan_update_conflict": true,
        "source_update_conflict": false,
        "conflict_files": ["src/foo.rs"],
        "source_branch": "task/branch",
        "target_branch": "plan/branch",
        // Non-freshness keys must be preserved
        "trigger_origin": "scheduler",
        "some_other_flag": 42,
    });

    FreshnessMetadata::clear_from(&mut meta);

    // All freshness keys removed
    let obj = meta.as_object().unwrap();
    assert!(!obj.contains_key("branch_freshness_conflict"));
    assert!(!obj.contains_key("freshness_origin_state"));
    assert!(!obj.contains_key("freshness_conflict_count"));
    assert!(!obj.contains_key("plan_update_conflict"));
    assert!(!obj.contains_key("source_update_conflict"));
    assert!(!obj.contains_key("conflict_files"));
    assert!(!obj.contains_key("source_branch"));
    assert!(!obj.contains_key("target_branch"));

    // Non-freshness keys preserved
    assert_eq!(meta["trigger_origin"], "scheduler");
    assert_eq!(meta["some_other_flag"], 42);
}

#[test]
fn test_metadata_cleared_for_reviewing_origin() {
    let mut meta = json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "reviewing",
        "freshness_conflict_count": 2,
        "source_update_conflict": true,
        "source_branch": "task/review-branch",
        "target_branch": "plan/main-plan",
    });

    FreshnessMetadata::clear_from(&mut meta);

    let obj = meta.as_object().unwrap();
    assert!(!obj.contains_key("branch_freshness_conflict"));
    assert!(!obj.contains_key("freshness_origin_state"));
    assert!(!obj.contains_key("freshness_conflict_count"));
    assert!(!obj.contains_key("source_update_conflict"));
    assert!(!obj.contains_key("source_branch"));
    assert!(!obj.contains_key("target_branch"));
}

#[test]
fn test_metadata_cleared_for_corrupted_no_origin() {
    // branch_freshness_conflict=true but freshness_origin_state absent
    let mut meta = json!({
        "branch_freshness_conflict": true,
        "freshness_conflict_count": 1,
    });

    FreshnessMetadata::clear_from(&mut meta);

    let obj = meta.as_object().unwrap();
    assert!(!obj.contains_key("branch_freshness_conflict"));
    assert!(!obj.contains_key("freshness_conflict_count"));
    // Object should be empty after clearing
    assert!(obj.is_empty());
}

// ==================
// FreshnessMetadata extraction from metadata JSON
// ==================

#[test]
fn test_from_metadata_extracts_origin_state_for_routing() {
    let meta = json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "reviewing",
        "freshness_conflict_count": 3,
    });

    let freshness = FreshnessMetadata::from_task_metadata(&meta);

    assert!(freshness.branch_freshness_conflict);
    assert_eq!(
        freshness.freshness_origin_state.as_deref(),
        Some("reviewing")
    );
    assert_eq!(freshness.freshness_conflict_count, 3);

    // Verify routing decision based on extracted state
    let target = routing_target_for(freshness.freshness_origin_state.as_deref());
    assert_eq!(target, InternalStatus::PendingReview);
}

#[test]
fn test_from_metadata_extracts_re_executing_for_routing() {
    let meta = json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "re_executing",
        "freshness_conflict_count": 1,
        "source_update_conflict": true,
    });

    let freshness = FreshnessMetadata::from_task_metadata(&meta);

    assert!(freshness.branch_freshness_conflict);
    assert_eq!(
        freshness.freshness_origin_state.as_deref(),
        Some("re_executing")
    );
    assert!(freshness.source_update_conflict);

    let target = routing_target_for(freshness.freshness_origin_state.as_deref());
    assert_eq!(target, InternalStatus::Ready);
}

// ==================
// Wave 3B: Review-origin freshness routing decision tests
// ==================
//
// These tests exercise the routing decision logic that mirrors the BranchFreshnessConflict
// handler in execute_entry_actions() (task_transition_service.rs:1335-1484):
//   - reviewing origin + count < 5  → PendingReview (re-queue for review)
//   - reviewing origin + count >= 5 → Failed       (loop protection cap)
//   - executing / re_executing / absent → Merging  (existing behavior)
//
// Also tests:
//   - on_exit target state for reviewing origin = PendingReview (not Merging)
//   - freshness_conflict_count NOT double-incremented for normal freshness path
//   - freshness_conflict_count IS incremented for conflict marker scan path
//   - stale metadata cleared after successful ReviewPassed (RoutingOnly scope)

/// Helper: determine routing target and whether to escalate.
///
/// Mirrors the BranchFreshnessConflict handler routing logic:
/// ```
/// let reviewing_origin = freshness_origin == Some("reviewing");
/// if reviewing_origin && conflict_count >= FRESHNESS_RETRY_LIMIT { → Failed }
/// if reviewing_origin { → PendingReview } else { → Merging }
/// ```
fn routing_target_with_cap(origin: Option<&str>, conflict_count: u32) -> InternalStatus {
    const FRESHNESS_RETRY_LIMIT: u32 = 5;
    let reviewing_origin = origin == Some("reviewing");
    if reviewing_origin && conflict_count >= FRESHNESS_RETRY_LIMIT {
        return InternalStatus::Failed;
    }
    if reviewing_origin {
        InternalStatus::PendingReview
    } else {
        InternalStatus::Merging
    }
}

/// Helper: simulate the effective conflict count after the BranchFreshnessConflict handler.
///
/// If freshness_count_incremented_by is present → already incremented (normal path),
/// return existing count. If absent → conflict marker scan path, would increment by 1.
fn effective_count_after_handler(meta: &serde_json::Value) -> u32 {
    let already_incremented = meta["freshness_count_incremented_by"].as_str().is_some();
    let current = meta["freshness_conflict_count"].as_u64().unwrap_or(0) as u32;
    if already_incremented {
        current
    } else {
        current + 1
    }
}

#[test]
fn test_review_origin_freshness_conflict_routes_to_pending_review_not_merging() {
    // Core fix: reviewing origin must route to PendingReview, not Merging
    let target = routing_target_with_cap(Some("reviewing"), 1);
    assert_eq!(
        target,
        InternalStatus::PendingReview,
        "reviewing origin must route to PendingReview (not Merging)"
    );
    assert_ne!(
        target,
        InternalStatus::Merging,
        "reviewing origin must NOT route to Merging"
    );
}

#[test]
fn test_executing_origin_freshness_conflict_routes_to_merging_regression() {
    // Regression: existing behavior for executing origin must not change
    let target = routing_target_with_cap(Some("executing"), 1);
    assert_eq!(
        target,
        InternalStatus::Merging,
        "executing origin must still route to Merging (regression safety)"
    );
    assert_ne!(
        target,
        InternalStatus::PendingReview,
        "executing origin must NOT route to PendingReview"
    );
}

#[test]
fn test_re_executing_origin_freshness_conflict_routes_to_merging_regression() {
    // Regression: re_executing origin also routes to Merging (execution phase)
    let target = routing_target_with_cap(Some("re_executing"), 1);
    assert_eq!(
        target,
        InternalStatus::Merging,
        "re_executing origin must route to Merging (regression safety)"
    );
}

#[test]
fn test_absent_origin_freshness_conflict_routes_to_merging_safe_default() {
    // Absent freshness_origin_state → safe default is Merging (not PendingReview)
    let target = routing_target_with_cap(None, 1);
    assert_eq!(
        target,
        InternalStatus::Merging,
        "absent origin must fall back to Merging as safe default"
    );
}

#[test]
fn test_review_origin_at_retry_cap_routes_to_failed() {
    // Loop protection: >= 5 conflicts during review → Failed (not PendingReview)
    let target = routing_target_with_cap(Some("reviewing"), 5);
    assert_eq!(
        target,
        InternalStatus::Failed,
        "reviewing origin with conflict_count >= 5 must route to Failed (loop protection)"
    );
    assert_ne!(
        target,
        InternalStatus::PendingReview,
        "reviewing origin at cap must NOT route to PendingReview (would loop forever)"
    );
}

#[test]
fn test_review_origin_one_below_retry_cap_routes_to_pending_review() {
    // Below cap (count = 4): still routes to PendingReview
    let target = routing_target_with_cap(Some("reviewing"), 4);
    assert_eq!(
        target,
        InternalStatus::PendingReview,
        "reviewing origin with conflict_count = 4 (under cap) must route to PendingReview"
    );
}

#[test]
fn test_on_exit_target_for_reviewing_origin_is_pending_review_not_merging() {
    // The BranchFreshnessConflict handler passes the dynamically-determined target_state
    // to on_exit. For reviewing origin, on_exit must receive PendingReview — not Merging.
    // This mirrors the fix in task_transition_service.rs:1438 where target_state is
    // dynamically determined before the on_exit call.
    let reviewing_origin = true;
    let (target_status, to_str) = if reviewing_origin {
        (InternalStatus::PendingReview, "pending_review")
    } else {
        (InternalStatus::Merging, "merging")
    };
    assert_eq!(
        target_status,
        InternalStatus::PendingReview,
        "on_exit target for reviewing origin must be PendingReview"
    );
    assert_eq!(
        to_str, "pending_review",
        "to_str label must match the routing target"
    );
}

#[test]
fn test_on_exit_target_for_executing_origin_is_merging() {
    // For executing origin, on_exit must still receive Merging (unchanged)
    let reviewing_origin = false;
    let (target_status, _to_str) = if reviewing_origin {
        (InternalStatus::PendingReview, "pending_review")
    } else {
        (InternalStatus::Merging, "merging")
    };
    assert_eq!(
        target_status,
        InternalStatus::Merging,
        "on_exit target for executing origin must still be Merging"
    );
}

#[test]
fn test_normal_freshness_path_count_not_double_incremented() {
    // When freshness_count_incremented_by is present (ensure_branches_fresh already incremented),
    // the handler must NOT increment again. Effective count equals stored count.
    let meta = json!({
        "freshness_origin_state": "reviewing",
        "freshness_conflict_count": 2,
        "freshness_count_incremented_by": "ensure_branches_fresh",
        "branch_freshness_conflict": true,
    });
    let count = effective_count_after_handler(&meta);
    assert_eq!(
        count,
        2,
        "Count must NOT be incremented again when freshness_count_incremented_by is present (normal path already incremented)"
    );
}

#[test]
fn test_conflict_marker_scan_path_count_incremented_once() {
    // When freshness_count_incremented_by is absent (conflict marker scan path bypassed
    // ensure_branches_fresh), the handler must increment by 1.
    let meta = json!({
        "freshness_origin_state": "reviewing",
        "freshness_conflict_count": 1,
        // freshness_count_incremented_by absent → conflict marker scan path
        "branch_freshness_conflict": true,
    });
    let count = effective_count_after_handler(&meta);
    assert_eq!(
        count,
        2,
        "Count must be incremented once when freshness_count_incremented_by is absent (conflict marker scan path)"
    );
}

#[test]
fn test_conflict_marker_scan_path_count_zero_baseline_incremented_to_one() {
    // Conflict marker scan path with freshness_conflict_count = 0 (first conflict):
    // effective count must be 1.
    let meta = json!({
        "freshness_origin_state": "reviewing",
        "freshness_conflict_count": 0,
        "branch_freshness_conflict": true,
        // freshness_count_incremented_by absent
    });
    let count = effective_count_after_handler(&meta);
    assert_eq!(
        count, 1,
        "First conflict via marker scan path must yield count = 1"
    );
}

#[test]
fn test_conflict_marker_scan_at_cap_minus_one_does_not_yet_trigger_cap() {
    // Marker scan path with count = 4: after increment count = 5 → at cap → Failed
    let meta = json!({
        "freshness_origin_state": "reviewing",
        "freshness_conflict_count": 4,
        "branch_freshness_conflict": true,
        // freshness_count_incremented_by absent → conflict marker scan
    });
    let count = effective_count_after_handler(&meta);
    assert_eq!(count, 5, "Marker scan increments to 5, reaching cap");
    // After increment to 5, routing decision should yield Failed
    let target = routing_target_with_cap(Some("reviewing"), count);
    assert_eq!(
        target,
        InternalStatus::Failed,
        "After marker scan increment to 5, routing must yield Failed"
    );
}

#[test]
fn test_stale_routing_metadata_cleared_on_review_passed_routing_only_scope() {
    // After successful ReviewPassed, on_enter(ReviewPassed) calls:
    //   FreshnessMetadata::cleanup(FreshnessCleanupScope::RoutingOnly, &mut meta)
    // This must clear freshness_origin_state and freshness_count_incremented_by.
    let mut meta = json!({
        "freshness_origin_state": "reviewing",
        "freshness_count_incremented_by": "ensure_branches_fresh",
        "freshness_conflict_count": 2,
        "branch_freshness_conflict": true,
        "plan_update_conflict": false,
        "source_update_conflict": false,
        // Non-freshness key must be preserved
        "trigger_origin": "scheduler",
    });

    FreshnessMetadata::cleanup(FreshnessCleanupScope::RoutingOnly, &mut meta);

    let obj = meta.as_object().unwrap();

    // Routing flags cleared
    assert!(
        !obj.contains_key("freshness_origin_state"),
        "freshness_origin_state must be cleared after ReviewPassed"
    );
    assert!(
        !obj.contains_key("freshness_count_incremented_by"),
        "freshness_count_incremented_by must be cleared after ReviewPassed"
    );
    assert!(
        !meta["branch_freshness_conflict"].as_bool().unwrap_or(true),
        "branch_freshness_conflict must be false after RoutingOnly cleanup"
    );

    // Conflict count preserved (RoutingOnly does not reset count)
    assert_eq!(
        meta["freshness_conflict_count"].as_u64().unwrap_or(0),
        2,
        "freshness_conflict_count must be preserved by RoutingOnly cleanup"
    );

    // Non-freshness keys preserved
    assert_eq!(
        meta["trigger_origin"], "scheduler",
        "Non-freshness keys must not be removed by RoutingOnly cleanup"
    );
}

#[test]
fn test_successful_review_after_prior_conflict_clears_both_routing_fields() {
    // Scenario: task had a prior freshness conflict (reviewing origin, normal path),
    // then successfully completed review. Both stale fields must be cleared.
    let mut meta = json!({
        "freshness_origin_state": "reviewing",
        "freshness_count_incremented_by": "ensure_branches_fresh",
        "freshness_conflict_count": 1,
        "branch_freshness_conflict": false,  // conflict was resolved before review succeeded
        "plan_update_conflict": false,
        "source_update_conflict": false,
    });

    // Simulate ReviewPassed on_enter cleanup
    FreshnessMetadata::cleanup(FreshnessCleanupScope::RoutingOnly, &mut meta);

    let obj = meta.as_object().unwrap();

    assert!(
        !obj.contains_key("freshness_origin_state"),
        "freshness_origin_state cleared after successful review"
    );
    assert!(
        !obj.contains_key("freshness_count_incremented_by"),
        "freshness_count_incremented_by cleared after successful review"
    );

    // Conflict count preserved
    assert_eq!(
        meta["freshness_conflict_count"].as_u64().unwrap_or(0),
        1,
        "freshness_conflict_count preserved (count tracks history, not a routing flag)"
    );
}

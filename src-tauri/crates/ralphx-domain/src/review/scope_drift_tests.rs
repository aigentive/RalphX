use super::*;
use crate::entities::TaskId;

#[test]
fn parses_scope_drift_classification() {
    assert_eq!(
        "adjacent_scope_expansion".parse::<ScopeDriftClassification>().unwrap(),
        ScopeDriftClassification::AdjacentScopeExpansion
    );
    assert_eq!(
        "plan_correction".parse::<ScopeDriftClassification>().unwrap(),
        ScopeDriftClassification::PlanCorrection
    );
    assert_eq!(
        "unrelated_drift".parse::<ScopeDriftClassification>().unwrap(),
        ScopeDriftClassification::UnrelatedDrift
    );
}

#[test]
fn rejects_invalid_scope_drift_classification() {
    let err = "bad".parse::<ScopeDriftClassification>().unwrap_err();
    assert!(err.to_string().contains("invalid scope drift classification"));
}

#[test]
fn computes_scope_drift_against_prefix_scope() {
    let changed = vec![
        "src/foo.rs".to_string(),
        "./src/nested/bar.rs".to_string(),
        "docs/readme.md".to_string(),
    ];
    let planned = vec!["src".to_string()];

    let (status, out_of_scope) = compute_scope_drift(&changed, &planned);

    assert_eq!(status, ScopeDriftStatus::ScopeExpansion);
    assert_eq!(out_of_scope, vec!["docs/readme.md".to_string()]);
}

#[test]
fn fingerprint_is_stable_for_same_files_regardless_of_order() {
    let task_id = TaskId::from_string("task-123".to_string());
    let a = compute_out_of_scope_blocker_fingerprint(
        &task_id,
        &["b.rs".to_string(), "a.rs".to_string(), "a.rs".to_string()],
    );
    let b = compute_out_of_scope_blocker_fingerprint(
        &task_id,
        &["a.rs".to_string(), "b.rs".to_string()],
    );

    assert_eq!(a, b);
}

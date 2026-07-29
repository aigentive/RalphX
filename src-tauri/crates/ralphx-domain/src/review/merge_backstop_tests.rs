use crate::entities::ReviewScopeMetadata;

use super::evaluate_merge_scope_backstop;

fn sample_review_scope() -> ReviewScopeMetadata {
    ReviewScopeMetadata::new(
        vec!["src-tauri/src/http_server/handlers/reviews".to_string()],
        vec!["ralphx.yaml".to_string()],
        Some("adjacent_scope_expansion".to_string()),
        Some("reviewed".to_string()),
    )
}

#[test]
fn merge_backstop_allows_when_changed_files_stay_within_reviewed_scope() {
    let review_scope = sample_review_scope();
    let changed_files = vec![
        "src-tauri/src/http_server/handlers/reviews/complete.rs".to_string(),
        "ralphx.yaml".to_string(),
    ];

    let violation = evaluate_merge_scope_backstop(&review_scope, &changed_files);

    assert!(violation.is_none());
}

#[test]
fn merge_backstop_blocks_when_review_never_classified_scope_expansion() {
    let mut review_scope = sample_review_scope();
    review_scope.drift_classification = None;
    let changed_files = vec!["ralphx.yaml".to_string()];

    let violation = evaluate_merge_scope_backstop(&review_scope, &changed_files)
        .expect("unclassified scope expansion should block merge");

    assert!(violation.reason.contains("never recorded a drift classification"));
    assert_eq!(violation.out_of_scope_files, vec!["ralphx.yaml".to_string()]);
}

#[test]
fn merge_backstop_blocks_unrelated_drift_even_if_reviewed() {
    let mut review_scope = sample_review_scope();
    review_scope.drift_classification = Some("unrelated_drift".to_string());
    let changed_files = vec!["ralphx.yaml".to_string()];

    let violation = evaluate_merge_scope_backstop(&review_scope, &changed_files)
        .expect("unrelated drift should block merge");

    assert!(violation.reason.contains("unrelated scope drift"));
}

#[test]
fn merge_backstop_blocks_new_unreviewed_scope_expansion() {
    let review_scope = sample_review_scope();
    let changed_files = vec![
        "ralphx.yaml".to_string(),
        "docs/new-surface.md".to_string(),
    ];

    let violation = evaluate_merge_scope_backstop(&review_scope, &changed_files)
        .expect("new unreviewed drift should block merge");

    assert!(violation.reason.contains("without fresh classification"));
    assert_eq!(violation.out_of_scope_files, vec!["docs/new-surface.md".to_string()]);
}

#[test]
fn merge_backstop_blocks_unknown_classification() {
    let mut review_scope = sample_review_scope();
    review_scope.drift_classification = Some("mystery".to_string());
    let changed_files = vec!["ralphx.yaml".to_string()];

    let violation = evaluate_merge_scope_backstop(&review_scope, &changed_files)
        .expect("unknown drift classification should block merge");

    assert!(violation.reason.contains("unsupported review scope drift classification"));
}

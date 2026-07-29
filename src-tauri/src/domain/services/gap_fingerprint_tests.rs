use std::collections::HashSet;

use super::*;

#[test]
fn test_gap_fingerprint_order_independence() {
    // "missing authentication" and "authentication missing" should produce the same fingerprint
    // after stop-word strip + sort
    let fp1 = gap_fingerprint("missing authentication");
    let fp2 = gap_fingerprint("authentication missing");
    assert_eq!(fp1, fp2, "order-independent fingerprints should match");
}

#[test]
fn test_gap_fingerprint_length() {
    let fp = gap_fingerprint("some gap description");
    assert_eq!(fp.len(), 12, "fingerprint should be 12 hex chars");
}

#[test]
fn test_gap_fingerprint_stop_word_stripping() {
    // Stop words should be stripped but not negation words
    let fp1 = gap_fingerprint("the authentication is missing");
    let fp2 = gap_fingerprint("authentication missing");
    assert_eq!(fp1, fp2, "stop words should be stripped");
}

#[test]
fn test_gap_fingerprint_preserves_negation() {
    // "not" should NOT be stripped
    let fp1 = gap_fingerprint("not authenticated");
    let fp2 = gap_fingerprint("authenticated");
    assert_ne!(fp1, fp2, "negation words must be preserved");
}

#[test]
fn test_jaccard_identical_sets() {
    let set_a: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
    let set_b = set_a.clone();
    assert!((jaccard_similarity(&set_a, &set_b) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_jaccard_empty_sets() {
    let set_a: HashSet<String> = HashSet::new();
    let set_b: HashSet<String> = HashSet::new();
    assert!((jaccard_similarity(&set_a, &set_b) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_jaccard_disjoint_sets() {
    let set_a: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
    let set_b: HashSet<String> = ["c", "d"].iter().map(|s| s.to_string()).collect();
    assert!((jaccard_similarity(&set_a, &set_b) - 0.0).abs() < f64::EPSILON);
}

#[test]
fn test_gap_score_mixed_severities() {
    use crate::domain::entities::ideation::VerificationGap;

    let gaps = vec![
        VerificationGap {
            severity: "critical".to_string(),
            category: "security".to_string(),
            description: "No auth".to_string(),
            why_it_matters: None,
            source: None,
        },
        VerificationGap {
            severity: "high".to_string(),
            category: "arch".to_string(),
            description: "Missing layer".to_string(),
            why_it_matters: None,
            source: None,
        },
        VerificationGap {
            severity: "medium".to_string(),
            category: "ux".to_string(),
            description: "Unclear flow".to_string(),
            why_it_matters: None,
            source: None,
        },
    ];
    assert_eq!(gap_score(&gaps), 14, "critical(10) + high(3) + medium(1) = 14");
}

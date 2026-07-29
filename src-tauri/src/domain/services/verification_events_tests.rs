use super::build_verification_payload;

use crate::domain::entities::{
    VerificationGap, VerificationRoundSnapshot, VerificationRunSnapshot, VerificationStatus,
};

// ===== None snapshot (B2, B3, B4 paths) =====

#[test]
fn test_none_metadata_produces_empty_arrays() {
    let payload = build_verification_payload(
        "sess-1",
        VerificationStatus::Unverified,
        false,
        None,
        None,
        None,
    );

    assert_eq!(payload["session_id"], "sess-1");
    assert_eq!(payload["status"], "unverified");
    assert_eq!(payload["in_progress"], false);
    assert_eq!(payload["generation"], serde_json::Value::Null);
    assert_eq!(payload["round"], serde_json::Value::Null);
    assert_eq!(payload["max_rounds"], serde_json::Value::Null);
    assert_eq!(payload["gap_score"], serde_json::Value::Null);
    assert_eq!(payload["convergence_reason"], serde_json::Value::Null);
    assert_eq!(payload["current_gaps"], serde_json::Value::Array(vec![]));
    assert_eq!(payload["rounds"], serde_json::Value::Array(vec![]));
}

#[test]
fn test_none_metadata_with_convergence_reason_override() {
    let payload = build_verification_payload(
        "sess-2",
        VerificationStatus::Skipped,
        false,
        None,
        Some("user_reverted"),
        Some(7),
    );

    assert_eq!(payload["status"], "skipped");
    assert_eq!(payload["generation"], 7);
    assert_eq!(payload["convergence_reason"], "user_reverted");
    assert_eq!(payload["current_gaps"], serde_json::Value::Array(vec![]));
    assert_eq!(payload["rounds"], serde_json::Value::Array(vec![]));
}

// ===== Some snapshot (B1 backend path) =====

#[test]
fn test_some_snapshot_includes_gaps_and_rounds() {
    let snapshot = VerificationRunSnapshot {
        generation: 3,
        status: VerificationStatus::NeedsRevision,
        in_progress: false,
        current_round: 2,
        max_rounds: 5,
        current_gaps: vec![
            VerificationGap {
                severity: "critical".to_string(),
                category: "security".to_string(),
                description: "Missing auth".to_string(),
                why_it_matters: None,
                source: None,
            },
            VerificationGap {
                severity: "high".to_string(),
                category: "architecture".to_string(),
                description: "No error handling".to_string(),
                why_it_matters: Some("Causes crashes".to_string()),
                source: None,
            },
            VerificationGap {
                severity: "medium".to_string(),
                category: "testing".to_string(),
                description: "Low coverage".to_string(),
                why_it_matters: None,
                source: None,
            },
        ],
        rounds: vec![
            VerificationRoundSnapshot {
                round: 1,
                fingerprints: vec!["fp1".to_string()],
                gap_score: 13,
                gaps: vec![],
                parse_failed: false,
            },
            VerificationRoundSnapshot {
                round: 2,
                fingerprints: vec!["fp2".to_string()],
                gap_score: 14,
                gaps: vec![],
                parse_failed: false,
            },
        ],
        convergence_reason: Some("max_rounds".to_string()),
        best_round_index: None,
    };

    let payload = build_verification_payload(
        "sess-3",
        VerificationStatus::NeedsRevision,
        false,
        Some(&snapshot),
        None,
        Some(3),
    );

    assert_eq!(payload["session_id"], "sess-3");
    assert_eq!(payload["status"], "needs_revision");
    assert_eq!(payload["generation"], 3);
    assert_eq!(payload["round"], 2);
    assert_eq!(payload["max_rounds"], 5);
    // critical*10 + high*3 + medium*1 = 10 + 3 + 1 = 14
    assert_eq!(payload["gap_score"], 14);
    assert_eq!(payload["convergence_reason"], "max_rounds");

    // Gaps array should have 3 entries
    let gaps = payload["current_gaps"].as_array().unwrap();
    assert_eq!(gaps.len(), 3);
    assert_eq!(gaps[0]["severity"], "critical");
    assert_eq!(gaps[0]["description"], "Missing auth");
    assert_eq!(gaps[1]["why_it_matters"], "Causes crashes");

    // Rounds array should have 2 entries
    let rounds = payload["rounds"].as_array().unwrap();
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[0]["gap_score"], 13);
    assert_eq!(rounds[1]["gap_score"], 14);
}

#[test]
fn test_some_snapshot_convergence_reason_override_wins() {
    let snapshot = VerificationRunSnapshot {
        generation: 4,
        status: VerificationStatus::Verified,
        in_progress: false,
        convergence_reason: Some("from_metadata".to_string()),
        current_round: 0,
        max_rounds: 0,
        current_gaps: vec![],
        rounds: vec![],
        best_round_index: None,
    };

    let payload = build_verification_payload(
        "sess-4",
        VerificationStatus::Verified,
        false,
        Some(&snapshot),
        Some("explicit_override"),
        Some(4),
    );

    // Explicit override takes precedence over metadata's value
    assert_eq!(payload["convergence_reason"], "explicit_override");
}

#[test]
fn test_some_snapshot_empty_gaps_and_rounds() {
    let snapshot = VerificationRunSnapshot {
        generation: 5,
        status: VerificationStatus::Reviewing,
        in_progress: true,
        current_round: 0,
        max_rounds: 0,
        current_gaps: vec![],
        rounds: vec![],
        convergence_reason: None,
        best_round_index: None,
    };

    let payload = build_verification_payload(
        "sess-5",
        VerificationStatus::Reviewing,
        true,
        Some(&snapshot),
        None,
        Some(5),
    );

    assert_eq!(payload["generation"], 5);
    assert_eq!(payload["gap_score"], 0u32);
    assert_eq!(payload["convergence_reason"], serde_json::Value::Null);
    assert_eq!(payload["current_gaps"], serde_json::Value::Array(vec![]));
    assert_eq!(payload["rounds"], serde_json::Value::Array(vec![]));
}

#[test]
fn test_gap_score_weighted_calculation() {
    // critical*10 + high*3 + medium*1 + unknown*0
    let snapshot = VerificationRunSnapshot {
        generation: 0,
        status: VerificationStatus::NeedsRevision,
        in_progress: false,
        current_gaps: vec![
            VerificationGap {
                severity: "critical".to_string(),
                category: "c".to_string(),
                description: "d".to_string(),
                why_it_matters: None,
                source: None,
            },
            VerificationGap {
                severity: "critical".to_string(),
                category: "c".to_string(),
                description: "d".to_string(),
                why_it_matters: None,
                source: None,
            },
            VerificationGap {
                severity: "high".to_string(),
                category: "c".to_string(),
                description: "d".to_string(),
                why_it_matters: None,
                source: None,
            },
            VerificationGap {
                severity: "low".to_string(), // mapped to 0
                category: "c".to_string(),
                description: "d".to_string(),
                why_it_matters: None,
                source: None,
            },
        ],
        current_round: 0,
        max_rounds: 0,
        rounds: vec![],
        convergence_reason: None,
        best_round_index: None,
    };

    let payload = build_verification_payload(
        "sess-6",
        VerificationStatus::NeedsRevision,
        false,
        Some(&snapshot),
        None,
        None,
    );

    // 2*10 + 1*3 + 0*1 + 0 = 23
    assert_eq!(payload["gap_score"], 23u32);
}

// ===== Contract fixture test =====

/// Validates that the canonical payload shape matches tests/fixtures/verification_event.json.
/// This is a schema contract test — the fixture serves as the frontend's expected shape.
#[test]
fn test_fixture_schema_matches_canonical_payload() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/verification_event.json"
    );
    let fixture_str = std::fs::read_to_string(fixture_path)
        .expect("tests/fixtures/verification_event.json must exist");
    let fixture: serde_json::Value =
        serde_json::from_str(&fixture_str).expect("fixture must be valid JSON");

    // Verify all canonical top-level fields are present
    let required_fields = [
        "session_id",
        "status",
        "in_progress",
        "generation",
        "round",
        "max_rounds",
        "gap_score",
        "convergence_reason",
        "current_gaps",
        "rounds",
    ];
    for field in required_fields {
        assert!(
            fixture.get(field).is_some(),
            "fixture missing required field: {field}"
        );
    }

    // Verify current_gaps items have the expected shape
    let gaps = fixture["current_gaps"].as_array().unwrap();
    assert!(!gaps.is_empty(), "fixture must have at least one gap");
    for gap in gaps {
        assert!(gap.get("severity").is_some(), "gap missing severity");
        assert!(gap.get("category").is_some(), "gap missing category");
        assert!(gap.get("description").is_some(), "gap missing description");
        // why_it_matters is optional (may be null)
        assert!(
            gap.get("why_it_matters").is_some(),
            "gap missing why_it_matters key"
        );
    }

    // Verify rounds items have the expected shape
    let rounds = fixture["rounds"].as_array().unwrap();
    assert!(!rounds.is_empty(), "fixture must have at least one round");
    for round in rounds {
        assert!(
            round.get("fingerprints").is_some(),
            "round missing fingerprints"
        );
        assert!(round.get("gap_score").is_some(), "round missing gap_score");
    }

    // Payload size check: 15 gaps × 5 rounds must stay under 5KB
    let payload_bytes = fixture_str.len();
    assert!(
        payload_bytes < 5120,
        "fixture payload {payload_bytes}B exceeds 5KB limit — review payload size"
    );
}

/// Verifies build_verification_payload produces output matching the fixture's schema shape.
#[test]
fn test_payload_shape_matches_fixture_schema() {
    let snapshot = VerificationRunSnapshot {
        generation: 9,
        status: VerificationStatus::NeedsRevision,
        in_progress: false,
        current_round: 3,
        max_rounds: 5,
        best_round_index: None,
        current_gaps: vec![
            VerificationGap {
                severity: "critical".to_string(),
                category: "security".to_string(),
                description: "No authentication on admin endpoints".to_string(),
                why_it_matters: Some("Allows unauthorized access".to_string()),
                source: None,
            },
            VerificationGap {
                severity: "high".to_string(),
                category: "testing".to_string(),
                description: "Missing integration tests".to_string(),
                why_it_matters: None,
                source: None,
            },
        ],
        rounds: vec![VerificationRoundSnapshot {
            round: 1,
            fingerprints: vec!["no-auth-admin".to_string(), "missing-tests".to_string()],
            gap_score: 13,
            gaps: vec![],
            parse_failed: false,
        }],
        convergence_reason: None,
    };

    let payload = build_verification_payload(
        "sess-fixture-shape",
        VerificationStatus::NeedsRevision,
        false,
        Some(&snapshot),
        None,
        Some(9),
    );

    // All required fields present with correct types
    assert!(payload["session_id"].is_string());
    assert!(payload["status"].is_string());
    assert!(payload["in_progress"].is_boolean());
    assert!(payload["generation"].is_number());
    assert!(payload["round"].is_number());
    assert!(payload["max_rounds"].is_number());
    assert!(payload["gap_score"].is_number());
    assert!(payload["current_gaps"].is_array());
    assert!(payload["rounds"].is_array());
    // convergence_reason is null (None) in this case
    assert_eq!(payload["convergence_reason"], serde_json::Value::Null);

    // Gap fields
    let gaps = payload["current_gaps"].as_array().unwrap();
    assert_eq!(gaps.len(), 2);
    assert_eq!(gaps[0]["severity"], "critical");
    assert_eq!(gaps[0]["why_it_matters"], "Allows unauthorized access");
    assert_eq!(gaps[1]["why_it_matters"], serde_json::Value::Null);

    // Payload serialization size check
    let serialized = serde_json::to_string(&payload).unwrap();
    assert!(
        serialized.len() < 5120,
        "payload {}B exceeds 5KB limit",
        serialized.len()
    );
}

// ===== ImportedVerified Tests =====

/// ImportedVerified: build_verification_payload produces a valid payload (no panic).
/// The application-layer emit wrapper skips emission entirely for ImportedVerified, but
/// the domain payload builder itself must handle it correctly if called directly.
#[test]
fn test_imported_verified_payload_does_not_panic() {
    let payload = build_verification_payload(
        "sess-imported",
        VerificationStatus::ImportedVerified,
        false,
        None,
        None,
        None,
    );
    assert_eq!(payload["session_id"], "sess-imported");
    assert_eq!(payload["status"], "imported_verified");
    assert_eq!(payload["in_progress"], false);
    assert_eq!(payload["generation"], serde_json::Value::Null);
    // All numeric fields are null when snapshot is None
    assert_eq!(payload["round"], serde_json::Value::Null);
    assert_eq!(payload["max_rounds"], serde_json::Value::Null);
    assert_eq!(payload["gap_score"], serde_json::Value::Null);
    assert_eq!(payload["current_gaps"], serde_json::Value::Array(vec![]));
    assert_eq!(payload["rounds"], serde_json::Value::Array(vec![]));
}

/// ImportedVerified with snapshot: status field is serialized as "imported_verified".
#[test]
fn test_imported_verified_payload_with_snapshot() {
    let snapshot = VerificationRunSnapshot {
        generation: 11,
        status: VerificationStatus::ImportedVerified,
        in_progress: false,
        current_round: 0,
        max_rounds: 0,
        best_round_index: None,
        current_gaps: vec![],
        rounds: vec![],
        convergence_reason: None,
    };
    let payload = build_verification_payload(
        "sess-imported-meta",
        VerificationStatus::ImportedVerified,
        false,
        Some(&snapshot),
        None,
        Some(11),
    );
    assert_eq!(payload["status"], "imported_verified");
    assert_eq!(payload["generation"], 11);
    assert_eq!(payload["gap_score"], 0u32);
    assert_eq!(payload["current_gaps"], serde_json::Value::Array(vec![]));
}

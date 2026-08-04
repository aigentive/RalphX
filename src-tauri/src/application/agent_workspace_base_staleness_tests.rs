use super::agent_workspace_base_staleness::{
    classify_health_hold_disposition, BaseStalenessObservation, HealthHoldDisposition,
};
use crate::domain::services::github_service::PrMergeStateStatus;

fn classify(
    merge_state_status: Option<&PrMergeStateStatus>,
    observed_base_oid: Option<&str>,
    attempt_target_base_commit: Option<&str>,
    last_base_update_oid: Option<&str>,
) -> HealthHoldDisposition {
    classify_health_hold_disposition(BaseStalenessObservation {
        merge_state_status,
        observed_base_oid,
        attempt_target_base_commit,
        last_base_update_oid,
    })
}

#[test]
fn behind_with_matching_base_supersedes_for_base_update() {
    assert_eq!(
        classify(
            Some(&PrMergeStateStatus::Behind),
            Some("base-tip"),
            Some("base-tip"),
            None,
        ),
        HealthHoldDisposition::SupersedeForBaseUpdate {
            observed_base_oid: "base-tip".to_string(),
        }
    );
}

#[test]
fn clean_with_matching_base_retains() {
    assert_eq!(
        classify(
            Some(&PrMergeStateStatus::Clean),
            Some("base-tip"),
            Some("base-tip"),
            None,
        ),
        HealthHoldDisposition::Retain
    );
}

#[test]
fn unknown_merge_state_retains() {
    for status in [
        None,
        Some(&PrMergeStateStatus::Unknown),
        Some(&PrMergeStateStatus::Other("UNKNOWN".to_string())),
    ] {
        assert_eq!(
            classify(status, Some("base-tip"), Some("base-tip"), None),
            HealthHoldDisposition::Retain
        );
    }
}

#[test]
fn blocked_merge_state_retains() {
    assert_eq!(
        classify(
            Some(&PrMergeStateStatus::Blocked),
            Some("base-tip"),
            Some("base-tip"),
            None,
        ),
        HealthHoldDisposition::Retain
    );
}

#[test]
fn behind_at_advanced_base_still_supersedes_for_base_update() {
    assert_eq!(
        classify(
            Some(&PrMergeStateStatus::Behind),
            Some("base-after"),
            Some("base-before"),
            None,
        ),
        HealthHoldDisposition::SupersedeForBaseUpdate {
            observed_base_oid: "base-after".to_string(),
        }
    );
}

#[test]
fn advanced_base_without_behind_supersedes_for_new_evidence() {
    assert_eq!(
        classify(
            Some(&PrMergeStateStatus::Clean),
            Some("base-after"),
            Some("base-before"),
            None,
        ),
        HealthHoldDisposition::SupersedeForNewEvidence {
            observed_base_oid: "base-after".to_string(),
        }
    );
}

#[test]
fn behind_at_already_updated_tip_blocks() {
    assert_eq!(
        classify(
            Some(&PrMergeStateStatus::Behind),
            Some("base-tip"),
            Some("base-tip"),
            Some(" base-tip "),
        ),
        HealthHoldDisposition::BlockedStaleAfterUpdate {
            observed_base_oid: "base-tip".to_string(),
        }
    );
}

#[test]
fn empty_and_whitespace_oids_retain() {
    for observed in [None, Some(""), Some("  ")] {
        assert_eq!(
            classify(
                Some(&PrMergeStateStatus::Behind),
                observed,
                Some("base-tip"),
                None,
            ),
            HealthHoldDisposition::Retain
        );
    }
}

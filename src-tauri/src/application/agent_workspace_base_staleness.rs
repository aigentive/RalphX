use crate::domain::services::github_service::PrMergeStateStatus;

/// What the current PR-autofix health hold should do for the observed remote base state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HealthHoldDisposition {
    Retain,
    SupersedeForBaseUpdate { observed_base_oid: String },
    SupersedeForNewEvidence { observed_base_oid: String },
    BlockedStaleAfterUpdate { observed_base_oid: String },
}

pub(crate) struct BaseStalenessObservation<'a> {
    pub merge_state_status: Option<&'a PrMergeStateStatus>,
    pub observed_base_oid: Option<&'a str>,
    pub attempt_target_base_commit: Option<&'a str>,
    pub last_base_update_oid: Option<&'a str>,
}

fn normalized_oid(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub(crate) fn classify_health_hold_disposition(
    observation: BaseStalenessObservation<'_>,
) -> HealthHoldDisposition {
    let Some(observed_base_oid) = normalized_oid(observation.observed_base_oid) else {
        return HealthHoldDisposition::Retain;
    };

    let behind = matches!(
        observation.merge_state_status,
        Some(PrMergeStateStatus::Behind)
    );

    if behind && normalized_oid(observation.last_base_update_oid) == Some(observed_base_oid) {
        return HealthHoldDisposition::BlockedStaleAfterUpdate {
            observed_base_oid: observed_base_oid.to_string(),
        };
    }

    if behind {
        return HealthHoldDisposition::SupersedeForBaseUpdate {
            observed_base_oid: observed_base_oid.to_string(),
        };
    }

    if normalized_oid(observation.attempt_target_base_commit)
        .is_some_and(|targeted| targeted != observed_base_oid)
    {
        return HealthHoldDisposition::SupersedeForNewEvidence {
            observed_base_oid: observed_base_oid.to_string(),
        };
    }

    HealthHoldDisposition::Retain
}

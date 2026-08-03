use crate::domain::services::github_service::{PrHealth, PrHealthCheck};

/// A check conclusion that ends a job without success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CiFailureKind {
    /// Real product failure: a rerun cannot clear it.
    Deterministic,
    /// Infrastructure failure: a rerun is the correct action.
    Transient,
}

pub(crate) fn classify_check_conclusion(conclusion: &str) -> Option<CiFailureKind> {
    match conclusion.trim().to_ascii_lowercase().as_str() {
        "failure" | "failed" | "error" | "action_required" | "stale" => {
            Some(CiFailureKind::Deterministic)
        }
        "cancelled" | "canceled" | "timed_out" | "timedout" | "startup_failure" => {
            Some(CiFailureKind::Transient)
        }
        _ => None,
    }
}

pub(crate) fn check_is_in_flight(check: &PrHealthCheck) -> bool {
    let has_no_conclusion = check
        .conclusion
        .as_deref()
        .is_none_or(|conclusion| conclusion.trim().is_empty());
    let is_in_flight_status = check.status.as_deref().is_some_and(|status| {
        matches!(
            status.trim().to_ascii_lowercase().as_str(),
            "queued" | "in_progress" | "pending" | "waiting" | "requested"
        )
    });

    has_no_conclusion && is_in_flight_status
}

pub(crate) fn workflow_run_id(check: &PrHealthCheck) -> Option<i64> {
    let url = check.details_url.as_deref()?;
    url.split('/')
        .collect::<Vec<_>>()
        .windows(2)
        .find_map(|parts| {
            (parts[0] == "runs")
                .then(|| parts[1].parse::<i64>().ok())
                .flatten()
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CiHoldIdentity {
    pub head_oid: String,
    pub run_ids: Vec<i64>,
}

impl CiHoldIdentity {
    pub fn new(head_oid: &str, run_ids: impl IntoIterator<Item = i64>) -> Self {
        let mut run_ids = run_ids.into_iter().collect::<Vec<_>>();
        run_ids.sort_unstable();
        run_ids.dedup();

        Self {
            head_oid: head_oid.to_string(),
            run_ids,
        }
    }

    /// `ci-hold:v1:<head_oid>:<id>,<id>` — versioned so legacy rows fail to parse.
    pub fn to_fingerprint(&self) -> String {
        let run_ids = self
            .run_ids
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        format!("ci-hold:v1:{}:{run_ids}", self.head_oid)
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let mut parts = raw.split(':');
        let prefix = parts.next()?;
        let version = parts.next()?;
        let head_oid = parts.next()?;
        let run_ids = parts.next()?;
        if prefix != "ci-hold"
            || version != "v1"
            || head_oid.is_empty()
            || run_ids.is_empty()
            || parts.next().is_some()
        {
            return None;
        }

        let run_ids = run_ids
            .split(',')
            .map(str::parse::<i64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        (!run_ids.is_empty()).then(|| Self::new(head_oid, run_ids))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TransientCiPlan {
    MissingHead,
    NoObservedFailure,
    /// Formatted `name (conclusion)` strings for the rejection message.
    DeterministicFailures(Vec<String>),
    AwaitRuns(CiHoldIdentity),
    Rerun {
        run_ids: Vec<i64>,
        hold: CiHoldIdentity,
    },
}

pub(crate) fn transient_ci_rerun_plan(health: &PrHealth) -> TransientCiPlan {
    let Some(head_oid) = health
        .sync_state
        .head_ref_oid
        .as_deref()
        .filter(|head_oid| !head_oid.trim().is_empty())
    else {
        return TransientCiPlan::MissingHead;
    };

    let deterministic_failures = health
        .checks
        .iter()
        .filter_map(|check| {
            (check
                .conclusion
                .as_deref()
                .and_then(classify_check_conclusion)
                == Some(CiFailureKind::Deterministic))
            .then(|| {
                format!(
                    "{} ({})",
                    check.name,
                    check.conclusion.as_deref().unwrap_or_default()
                )
            })
        })
        .collect::<Vec<_>>();
    if !deterministic_failures.is_empty() {
        return TransientCiPlan::DeterministicFailures(deterministic_failures);
    }

    let mut transient_run_ids = health
        .checks
        .iter()
        .filter(|check| {
            check
                .conclusion
                .as_deref()
                .and_then(classify_check_conclusion)
                == Some(CiFailureKind::Transient)
        })
        .filter_map(workflow_run_id)
        .collect::<Vec<_>>();
    transient_run_ids.sort_unstable();
    transient_run_ids.dedup();
    if transient_run_ids.is_empty() {
        return TransientCiPlan::NoObservedFailure;
    }

    let in_flight_run_ids = transient_run_ids
        .iter()
        .copied()
        .filter(|run_id| {
            health
                .checks
                .iter()
                .any(|check| workflow_run_id(check) == Some(*run_id) && check_is_in_flight(check))
        })
        .collect::<Vec<_>>();
    let terminal_run_ids = transient_run_ids
        .into_iter()
        .filter(|run_id| !in_flight_run_ids.contains(run_id))
        .collect::<Vec<_>>();

    if !terminal_run_ids.is_empty() {
        return TransientCiPlan::Rerun {
            hold: CiHoldIdentity::new(head_oid, terminal_run_ids.iter().copied()),
            run_ids: terminal_run_ids,
        };
    }

    TransientCiPlan::AwaitRuns(CiHoldIdentity::new(head_oid, in_flight_run_ids))
}

/// True while the identified runs are still expected to change.
pub(crate) fn ci_rerun_hold_still_pending(health: &PrHealth, fingerprint: Option<&str>) -> bool {
    let Some(identity) = fingerprint.and_then(CiHoldIdentity::parse) else {
        return false;
    };
    if health.sync_state.head_ref_oid.as_deref() != Some(identity.head_oid.as_str()) {
        return false;
    }

    let matching_checks = health.checks.iter().filter(|check| {
        workflow_run_id(check).is_some_and(|run_id| identity.run_ids.contains(&run_id))
    });
    for check in matching_checks {
        if check_is_in_flight(check) {
            return true;
        }
    }

    false
}

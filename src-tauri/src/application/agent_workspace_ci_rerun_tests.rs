use super::agent_workspace_ci_rerun::{
    ci_rerun_hold_still_pending, classify_check_conclusion, transient_ci_rerun_plan, CiFailureKind,
    CiHoldIdentity, TransientCiPlan,
};
use crate::domain::services::github_service::{
    PrHealth, PrHealthCheck, PrMergeableState, PrStatus, PrSyncState,
};

fn health(head_oid: Option<&str>, checks: Vec<PrHealthCheck>) -> PrHealth {
    PrHealth {
        sync_state: PrSyncState {
            status: PrStatus::Open,
            merge_state_status: None,
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: "feature/transient-ci".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: head_oid.map(str::to_string),
            base_ref_oid: Some("base-oid".to_string()),
        },
        review_decision: None,
        checks,
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }
}

fn check(
    name: &str,
    status: Option<&str>,
    conclusion: Option<&str>,
    run_id: Option<i64>,
) -> PrHealthCheck {
    PrHealthCheck {
        name: name.to_string(),
        status: status.map(str::to_string),
        conclusion: conclusion.map(str::to_string),
        details_url: run_id
            .map(|id| format!("https://github.com/acme/ralphx/actions/runs/{id}/jobs/42")),
    }
}

#[test]
fn classify_check_conclusion_maps_every_known_conclusion() {
    for conclusion in ["failure", " failed ", "ERROR", "action_required", "stale"] {
        assert_eq!(
            classify_check_conclusion(conclusion),
            Some(CiFailureKind::Deterministic),
            "{conclusion:?} must remain a real product failure"
        );
    }

    for conclusion in [
        "cancelled",
        " CANCELED ",
        "timed_out",
        "TIMEDOUT",
        "startup_failure",
    ] {
        assert_eq!(
            classify_check_conclusion(conclusion),
            Some(CiFailureKind::Transient),
            "{conclusion:?} must remain rerunnable infrastructure failure"
        );
    }

    for conclusion in ["success", "neutral", "skipped", "", "unknown"] {
        assert_eq!(
            classify_check_conclusion(conclusion),
            None,
            "{conclusion:?}"
        );
    }
}

#[test]
fn plan_rejects_when_head_ref_oid_is_missing() {
    assert_eq!(
        transient_ci_rerun_plan(&health(None, Vec::new())),
        TransientCiPlan::MissingHead
    );
    assert_eq!(
        transient_ci_rerun_plan(&health(Some(""), Vec::new())),
        TransientCiPlan::MissingHead
    );
}

#[test]
fn plan_rejects_when_any_deterministic_failure_exists() {
    let health = health(
        Some("head-1"),
        vec![
            check("Rust tests", Some("completed"), Some("failure"), Some(7)),
            check(
                "Hosted runner",
                Some("completed"),
                Some("cancelled"),
                Some(8),
            ),
        ],
    );

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::DeterministicFailures(vec!["Rust tests (failure)".to_string()])
    );
}

#[test]
fn plan_rejects_deterministic_failure_listed_after_cancellation() {
    let health = health(
        Some("head-1"),
        vec![
            check(
                "Hosted runner",
                Some("completed"),
                Some("cancelled"),
                Some(8),
            ),
            check("Rust tests", Some("completed"), Some("failure"), Some(7)),
        ],
    );

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::DeterministicFailures(vec!["Rust tests (failure)".to_string()])
    );
}

#[test]
fn plan_reruns_when_deterministic_failure_shares_a_run_with_a_transient_sibling() {
    let health = health(
        Some("head-1"),
        vec![
            check("Rust tests", Some("completed"), Some("failure"), Some(42)),
            check(
                "Hosted runner",
                Some("completed"),
                Some("cancelled"),
                Some(42),
            ),
        ],
    );
    let hold = CiHoldIdentity::new("head-1", [42]);

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::Rerun {
            run_ids: vec![42],
            hold,
        }
    );
}

#[test]
fn plan_rejects_deterministic_failure_in_a_run_with_no_transient_sibling() {
    let health = health(
        Some("head-1"),
        vec![
            check("Rust tests", Some("completed"), Some("failure"), Some(7)),
            check(
                "Hosted runner",
                Some("completed"),
                Some("cancelled"),
                Some(8),
            ),
        ],
    );

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::DeterministicFailures(vec!["Rust tests (failure)".to_string()])
    );
}

#[test]
fn plan_rejects_deterministic_failure_with_unparseable_run_url() {
    let mut deterministic = check("Rust tests", Some("completed"), Some("failure"), None);
    deterministic.details_url = Some("https://github.com/acme/ralphx/actions/jobs/99".to_string());
    let health = health(
        Some("head-1"),
        vec![
            deterministic,
            check(
                "Hosted runner",
                Some("completed"),
                Some("cancelled"),
                Some(8),
            ),
        ],
    );

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::DeterministicFailures(vec!["Rust tests (failure)".to_string()])
    );
}

#[test]
fn plan_awaits_when_a_run_scoped_deterministic_failure_has_an_in_flight_transient_sibling() {
    let health = health(
        Some("head-1"),
        vec![
            check("Rust tests", Some("completed"), Some("failure"), Some(64)),
            check(
                "Hosted runner",
                Some("completed"),
                Some("cancelled"),
                Some(64),
            ),
            check("Hosted runner / job", Some("in_progress"), None, Some(64)),
        ],
    );
    let hold = CiHoldIdentity::new("head-1", [64]);

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::AwaitRuns(hold)
    );
}

#[test]
fn plan_awaits_when_the_transient_run_has_in_flight_jobs() {
    let health = health(
        Some("head-1"),
        vec![
            check(
                "Hosted runner",
                Some("completed"),
                Some("cancelled"),
                Some(976),
            ),
            check("Hosted runner / job", Some("in_progress"), None, Some(976)),
        ],
    );
    let hold = CiHoldIdentity::new("head-1", [976]);

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::AwaitRuns(hold)
    );
}

#[test]
fn plan_reruns_every_distinct_terminal_transient_run() {
    let health = health(
        Some("head-1"),
        vec![
            check("Runner one", Some("completed"), Some("cancelled"), Some(30)),
            check("Runner two", Some("completed"), Some("timed_out"), Some(10)),
            check(
                "Runner one duplicate",
                Some("completed"),
                Some("canceled"),
                Some(30),
            ),
        ],
    );
    let hold = CiHoldIdentity::new("head-1", [10, 30]);

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::Rerun {
            run_ids: vec![10, 30],
            hold,
        }
    );
}

#[test]
fn plan_reruns_only_terminal_runs_when_mixed() {
    let health = health(
        Some("head-1"),
        vec![
            check(
                "Completed runner",
                Some("completed"),
                Some("cancelled"),
                Some(10),
            ),
            check(
                "Still running",
                Some("completed"),
                Some("timed_out"),
                Some(20),
            ),
            check("Still running / job", Some("waiting"), None, Some(20)),
        ],
    );
    let hold = CiHoldIdentity::new("head-1", [10]);

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::Rerun {
            run_ids: vec![10],
            hold,
        }
    );
}

#[test]
fn plan_reports_nothing_to_rerun_for_all_green() {
    // A healthy head is not a blocked repair. This used to report `NoObservedFailure`, which the
    // caller settles by *blocking* the attempt — a second dead generation for a passing PR.
    let health = health(
        Some("head-1"),
        vec![check(
            "Rust tests",
            Some("completed"),
            Some("success"),
            Some(1),
        )],
    );

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::NoFailuresAtHead
    );
}

#[test]
fn plan_reports_nothing_to_rerun_when_no_checks_exist_at_all() {
    assert_eq!(
        transient_ci_rerun_plan(&health(Some("head-1"), Vec::new())),
        TransientCiPlan::NoFailuresAtHead
    );
}

/// The incident shape: a fixer classified `needs_human` with "rerun once the workflow completes"
/// because an in-progress run carried no conclusion. An in-flight run can never reach
/// `transient_run_ids` (that classifier requires a conclusion), so this previously fell through to
/// `NoObservedFailure` and blocked the attempt instead of waiting for the run.
#[test]
fn plan_awaits_in_flight_runs_that_have_not_concluded() {
    for status in ["queued", "in_progress", "pending", "waiting", "requested"] {
        let health = health(
            Some("head-1"),
            vec![check("Rust tests", Some(status), None, Some(77))],
        );

        assert_eq!(
            transient_ci_rerun_plan(&health),
            TransientCiPlan::AwaitRuns(CiHoldIdentity::new("head-1", [77])),
            "status {status:?} must hold for the run rather than block the attempt"
        );
    }
}

#[test]
fn plan_awaits_in_flight_runs_alongside_passing_checks() {
    let health = health(
        Some("head-1"),
        vec![
            check("Lint", Some("completed"), Some("success"), Some(10)),
            check("Rust tests", Some("in_progress"), None, Some(20)),
        ],
    );

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::AwaitRuns(CiHoldIdentity::new("head-1", [20]))
    );
}

/// A deterministic failure still vetoes, even while another run is in flight — waiting cannot
/// clear a real product failure.
#[test]
fn plan_still_rejects_deterministic_failures_while_a_run_is_in_flight() {
    let health = health(
        Some("head-1"),
        vec![
            check("Rust tests", Some("completed"), Some("failure"), Some(10)),
            check("Lint", Some("in_progress"), None, Some(20)),
        ],
    );

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::DeterministicFailures(vec!["Rust tests (failure)".to_string()])
    );
}

/// An in-flight check RalphX cannot name a run for is an unresolved signal, not a quiet head, so
/// it must stay a blocker rather than claim there is nothing to rerun.
#[test]
fn plan_reports_no_observed_failure_for_in_flight_checks_without_a_parsable_run_url() {
    let mut unparseable = check("Hosted runner", Some("in_progress"), None, None);
    unparseable.details_url = Some("https://github.com/acme/ralphx/actions/jobs/99".to_string());

    assert_eq!(
        transient_ci_rerun_plan(&health(Some("head-1"), vec![unparseable])),
        TransientCiPlan::NoObservedFailure
    );
}

/// Every health shape must leave the fixer at least one valid classification, or the guards trade
/// one stuck state for another. `NoFailuresAtHead` and `NoObservedFailure` reject or block
/// `transient_ci` but leave `fixed` / `needs_human` open; `AwaitRuns` accepts `transient_ci`.
#[test]
fn every_health_shape_retains_a_valid_classification() {
    let shapes = [
        (
            "all green",
            health(
                Some("head-1"),
                vec![check("Lint", Some("completed"), Some("success"), Some(1))],
            ),
        ),
        (
            "in flight",
            health(
                Some("head-1"),
                vec![check("Lint", Some("in_progress"), None, Some(1))],
            ),
        ),
        (
            "transient failure",
            health(
                Some("head-1"),
                vec![check("Lint", Some("completed"), Some("cancelled"), Some(1))],
            ),
        ),
        (
            "deterministic failure",
            health(
                Some("head-1"),
                vec![check("Lint", Some("completed"), Some("failure"), Some(1))],
            ),
        ),
    ];

    for (label, health) in shapes {
        let plan = transient_ci_rerun_plan(&health);
        let transient_ci_is_accepted = matches!(
            plan,
            TransientCiPlan::AwaitRuns(_) | TransientCiPlan::Rerun { .. }
        );
        let another_classification_remains = matches!(
            plan,
            TransientCiPlan::NoFailuresAtHead
                | TransientCiPlan::NoObservedFailure
                | TransientCiPlan::DeterministicFailures(_)
                | TransientCiPlan::MissingHead
        );
        assert!(
            transient_ci_is_accepted || another_classification_remains,
            "{label}: no classification path remains for this health shape"
        );
    }
}

/// The hold `AwaitRuns` creates must actually resume: the poller consults
/// `ci_rerun_hold_still_pending` with the stored fingerprint, waits while the run is in flight,
/// and stops waiting once it concludes — at which point the concluded transient run is rerunnable.
#[test]
fn an_in_flight_hold_resumes_once_the_run_concludes() {
    let in_flight = health(
        Some("head-1"),
        vec![check("Rust tests", Some("in_progress"), None, Some(77))],
    );
    let TransientCiPlan::AwaitRuns(hold) = transient_ci_rerun_plan(&in_flight) else {
        panic!("an in-flight run must produce a hold");
    };
    let fingerprint = hold.to_fingerprint();

    assert!(
        ci_rerun_hold_still_pending(&in_flight, Some(&fingerprint)),
        "the poller must keep waiting while the held run is in flight"
    );

    let concluded = health(
        Some("head-1"),
        vec![check(
            "Rust tests",
            Some("completed"),
            Some("cancelled"),
            Some(77),
        )],
    );
    assert!(
        !ci_rerun_hold_still_pending(&concluded, Some(&fingerprint)),
        "the poller must stop waiting once the held run concludes"
    );
    assert_eq!(
        transient_ci_rerun_plan(&concluded),
        TransientCiPlan::Rerun {
            hold: CiHoldIdentity::new("head-1", [77]),
            run_ids: vec![77],
        },
        "the concluded transient run must then be rerunnable"
    );
}

#[test]
fn plan_ignores_transient_checks_without_a_parsable_run_url() {
    let mut unparseable = check("Hosted runner", Some("completed"), Some("cancelled"), None);
    unparseable.details_url = Some("https://github.com/acme/ralphx/actions/jobs/99".to_string());
    let health = health(Some("head-1"), vec![unparseable]);

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::NoObservedFailure
    );
}

#[test]
fn hold_identity_fingerprint_round_trips_and_sorts() {
    let identity = CiHoldIdentity::new("head-1", [30, 10, 30, 20]);

    assert_eq!(identity.run_ids, vec![10, 20, 30]);
    assert_eq!(identity.to_fingerprint(), "ci-hold:v1:head-1:10,20,30");
    assert_eq!(
        CiHoldIdentity::parse(&identity.to_fingerprint()),
        Some(identity)
    );
}

#[test]
fn hold_pending_while_any_identified_run_is_in_flight() {
    let health = health(
        Some("head-1"),
        vec![
            check("Finished", Some("completed"), Some("cancelled"), Some(10)),
            check("Running", Some("queued"), None, Some(20)),
        ],
    );
    let fingerprint = CiHoldIdentity::new("head-1", [10, 20]).to_fingerprint();

    assert!(ci_rerun_hold_still_pending(&health, Some(&fingerprint)));
}

#[test]
fn hold_ends_when_all_identified_runs_are_terminal() {
    let health = health(
        Some("head-1"),
        vec![check(
            "Finished",
            Some("completed"),
            Some("cancelled"),
            Some(10),
        )],
    );
    let fingerprint = CiHoldIdentity::new("head-1", [10]).to_fingerprint();

    assert!(!ci_rerun_hold_still_pending(&health, Some(&fingerprint)));
}

#[test]
fn hold_ends_when_head_moved() {
    let health = health(
        Some("head-2"),
        vec![check("Running", Some("in_progress"), None, Some(10))],
    );
    let fingerprint = CiHoldIdentity::new("head-1", [10]).to_fingerprint();

    assert!(!ci_rerun_hold_still_pending(&health, Some(&fingerprint)));
}

#[test]
fn hold_ends_for_legacy_or_unparsable_fingerprint() {
    let health = health(
        Some("head-1"),
        vec![check("Running", Some("in_progress"), None, Some(10))],
    );

    assert!(!ci_rerun_hold_still_pending(
        &health,
        Some("head-1:CI / test:failure:https://github.com/acme/ralphx/actions/runs/10")
    ));
    assert!(!ci_rerun_hold_still_pending(
        &health,
        Some("ci-hold:v1:head-1:not-a-run")
    ));
    assert!(!ci_rerun_hold_still_pending(&health, None));
}

#[test]
fn hold_ends_when_identified_runs_are_absent_from_health() {
    let health = health(
        Some("head-1"),
        vec![check("Other run", Some("in_progress"), None, Some(20))],
    );
    let fingerprint = CiHoldIdentity::new("head-1", [10]).to_fingerprint();

    assert!(!ci_rerun_hold_still_pending(&health, Some(&fingerprint)));
}

#[test]
fn plan_rejects_whitespace_only_head_oid() {
    // The filter trims before checking empty; whitespace-only OIDs must be treated as
    // missing to avoid publishing a fingerprint with a nonsensical head.
    for whitespace in ["  ", "\t", "\n"] {
        assert_eq!(
            transient_ci_rerun_plan(&health(Some(whitespace), Vec::new())),
            TransientCiPlan::MissingHead,
            "whitespace OID {whitespace:?} must be treated as missing"
        );
    }
}

#[test]
fn hold_identity_parse_rejects_empty_head_oid_segment() {
    // "ci-hold:v1::10" has an empty head OID between the second and third colons.
    assert_eq!(CiHoldIdentity::parse("ci-hold:v1::10"), None);
}

#[test]
fn hold_ends_for_empty_string_fingerprint() {
    let health = health(
        Some("head-1"),
        vec![check("Running", Some("in_progress"), None, Some(10))],
    );
    assert!(!ci_rerun_hold_still_pending(&health, Some("")));
}

#[test]
fn plan_ignores_transient_check_with_no_details_url() {
    // A transient check with no details_url has no parseable run ID and is silently ignored.
    let health = health(
        Some("head-1"),
        vec![check(
            "Hosted runner",
            Some("completed"),
            Some("cancelled"),
            None,
        )],
    );

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::NoObservedFailure
    );
}

#[test]
fn plan_rejects_deterministic_failure_with_no_details_url() {
    // A deterministic check with no details_url is still a real failure regardless of URL.
    let health = health(
        Some("head-1"),
        vec![check(
            "Rust tests",
            Some("completed"),
            Some("failure"),
            None,
        )],
    );

    assert_eq!(
        transient_ci_rerun_plan(&health),
        TransientCiPlan::DeterministicFailures(vec!["Rust tests (failure)".to_string()])
    );
}

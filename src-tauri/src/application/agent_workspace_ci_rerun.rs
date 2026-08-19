use std::path::Path;
use std::sync::Arc;

use crate::domain::entities::AgentWorkspaceRepairAttempt;
use crate::domain::repositories::{AgentWorkspaceRepairRepository, BranchUpdateRepository};
use crate::domain::services::github_service::{GithubServiceTrait, PrHealth, PrHealthCheck};
use crate::error::{AppError, AppResult};

use super::agent_workspace_publish_repair_state::{
    block_agent_workspace_repair_completion, reserve_agent_workspace_ci_await,
    reserve_agent_workspace_ci_rerun, AgentWorkspaceRepairTransitionOutcome,
    MAX_AGENT_WORKSPACE_CI_RERUN_RETRIES,
};

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
    /// Something at this head is unresolved — a transient conclusion whose run id could not be
    /// parsed, or an in-flight check with no usable run URL — so RalphX cannot name a run to
    /// rerun. Distinct from [`TransientCiPlan::NoFailuresAtHead`]: here there *is* a signal and
    /// RalphX cannot act on it, which is a genuine blocker.
    NoObservedFailure,
    /// Nothing failed and nothing is running. A healthy PR is not a blocked repair, so this
    /// rejects the classification instead of burning a generation on a hold with no evidence.
    NoFailuresAtHead,
    /// Formatted `name (conclusion)` strings for the rejection message. Run-scoped: a
    /// deterministic conclusion inside a workflow run that also has a transient
    /// (`cancelled`/`timed_out`/`startup_failure`) sibling does not veto here — that run
    /// is treated as poisoned infrastructure and its deterministic check is excluded, so
    /// only checks in runs with no transient sibling ever land in this variant.
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

    let deterministic_failures = health
        .checks
        .iter()
        .filter(|check| {
            check
                .conclusion
                .as_deref()
                .and_then(classify_check_conclusion)
                == Some(CiFailureKind::Deterministic)
        })
        .filter(|check| workflow_run_id(check).is_none_or(|id| !transient_run_ids.contains(&id)))
        .map(|check| {
            format!(
                "{} ({})",
                check.name,
                check.conclusion.as_deref().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    if !deterministic_failures.is_empty() {
        return TransientCiPlan::DeterministicFailures(deterministic_failures);
    }

    if transient_run_ids.is_empty() {
        // `classify_check_conclusion` requires a conclusion and `check_is_in_flight` requires the
        // absence of one, so an in-flight run can never reach `transient_run_ids`. Before this
        // branch existed, a head whose only signal was an in-progress run fell through to
        // `NoObservedFailure` and blocked the attempt — which is what stranded a fixer that
        // correctly wanted to rerun a run once it finished. Hold for the run instead.
        let in_flight_run_ids = health
            .checks
            .iter()
            .filter(|check| check_is_in_flight(check))
            .filter_map(workflow_run_id)
            .collect::<Vec<_>>();
        if !in_flight_run_ids.is_empty() {
            return TransientCiPlan::AwaitRuns(CiHoldIdentity::new(head_oid, in_flight_run_ids));
        }
        // An unresolved signal RalphX cannot name a run for stays a blocker; only a genuinely
        // quiet head is reported as having nothing to rerun.
        let has_unresolved_signal = health.checks.iter().any(|check| {
            check_is_in_flight(check)
                || check
                    .conclusion
                    .as_deref()
                    .and_then(classify_check_conclusion)
                    == Some(CiFailureKind::Transient)
        });
        return if has_unresolved_signal {
            TransientCiPlan::NoObservedFailure
        } else {
            TransientCiPlan::NoFailuresAtHead
        };
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

/// Typed settlement of [`execute_transient_ci_rerun`]. Every variant already carries the exact
/// message an HTTP adapter renders, so the durable classification/reservation/triage sequence
/// stays the single source of truth for those strings instead of duplicating them per caller.
#[derive(Debug)]
pub(crate) enum TransientCiRerunOutcome {
    /// The GitHub health reload itself failed; distinct from the other variants because callers
    /// historically surface this as a different HTTP status than a durable-state settlement.
    HealthFetchFailed(AppError),
    Rejected(String),
    Blocked(String),
    RerunPending(String),
    /// A reservation CAS observed a stale or missing durable attempt. Callers that need the full
    /// current-authority projection must re-derive it themselves; this function only reports that
    /// the caller's exact generation is no longer current.
    ReservationStale,
}

/// Runs the durable transient-CI-rerun sequence: reload PR health, classify it, reserve the
/// matching durable state, and (for a `Rerun` classification) request the GitHub reruns and
/// triage any request-level errors. Parameterized by repos and the GitHub service so both the
/// repair-completion HTTP boundary and a direct user-initiated rerun command share one
/// implementation instead of re-deriving this sequence.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_transient_ci_rerun(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
    github: Arc<dyn GithubServiceTrait>,
    attempt: AgentWorkspaceRepairAttempt,
    working_dir: &Path,
    pr_number: i64,
    summary: &str,
    auto_merge_current: Option<bool>,
    what_happened: Option<&str>,
    what_i_did: Option<&str>,
) -> AppResult<TransientCiRerunOutcome> {
    let health = match github.fetch_pr_health(working_dir, pr_number).await {
        Ok(health) => health,
        Err(error) => return Ok(TransientCiRerunOutcome::HealthFetchFailed(error)),
    };

    match transient_ci_rerun_plan(&health) {
        TransientCiPlan::MissingHead => Ok(TransientCiRerunOutcome::Rejected(
            "RalphX could not read the current PR head from fresh GitHub health, so it cannot verify a transient CI classification. Re-check PR health and either fix the failure or report a blocker.".to_string(),
        )),
        TransientCiPlan::NoObservedFailure => {
            block_transient_ci_rerun_outcome(
                repair_repo,
                branch_update_repo,
                attempt,
                summary,
                "RalphX could not identify a failed GitHub Actions run from fresh PR health.",
                auto_merge_current,
                what_happened,
                what_i_did,
            )
            .await
        }
        TransientCiPlan::NoFailuresAtHead => Ok(TransientCiRerunOutcome::Rejected(
            "Rejected `transient_ci`: fresh PR health reports no failing and no in-progress checks at the current head, so there is no GitHub Actions run to rerun. If your change resolved the failure, complete with `fixed`; otherwise classify `pre_existing_on_base` / `needs_human`.".to_string(),
        )),
        TransientCiPlan::DeterministicFailures(checks) => Ok(TransientCiRerunOutcome::Rejected(format!(
            "Rejected `transient_ci`: PR health still reports real failing checks at the current head: {}. Rerunning cannot clear them. Fix the failure and complete with `fixed`, or classify `pre_existing_on_base` / `needs_human`.",
            checks.join(", ")
        ))),
        TransientCiPlan::AwaitRuns(hold) => {
            await_transient_ci_runs_outcome(
                repair_repo,
                attempt,
                summary,
                &hold,
                "RalphX is waiting for the in-progress GitHub Actions run to finish before rerunning it.",
                auto_merge_current,
                what_happened,
                what_i_did,
            )
            .await
        }
        TransientCiPlan::Rerun { run_ids, hold } => {
            if attempt.ci_rerun_count >= MAX_AGENT_WORKSPACE_CI_RERUN_RETRIES {
                return block_transient_ci_rerun_outcome(
                    repair_repo,
                    branch_update_repo,
                    attempt,
                    summary,
                    "The transient CI rerun budget is exhausted.",
                    auto_merge_current,
                    what_happened,
                    what_i_did,
                )
                .await;
            }
            let reserved = match reserve_agent_workspace_ci_rerun(
                Arc::clone(&repair_repo),
                attempt,
                &hold.to_fingerprint(),
                "Transient CI failure accepted; RalphX is rerunning failed GitHub Actions jobs.",
                auto_merge_current,
                what_happened,
                what_i_did,
            )
            .await?
            {
                AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
                AgentWorkspaceRepairTransitionOutcome::Stale(_)
                | AgentWorkspaceRepairTransitionOutcome::Missing => {
                    return Ok(TransientCiRerunOutcome::ReservationStale);
                }
            };

            let mut rerun_errors = Vec::new();
            for workflow_run_id in run_ids {
                if let Err(error) = github.rerun_failed_workflow(working_dir, workflow_run_id).await {
                    rerun_errors.push((github_rerun_error_is_transient(&error), error.to_string()));
                }
            }
            if rerun_errors.is_empty() {
                return Ok(TransientCiRerunOutcome::RerunPending(
                    "RalphX accepted the transient CI classification and reran the failed GitHub Actions jobs.".to_string(),
                ));
            }
            if let Some((_, error)) = rerun_errors.iter().find(|(transient, _)| !transient) {
                return block_transient_ci_rerun_outcome(
                    repair_repo,
                    branch_update_repo,
                    reserved,
                    summary,
                    &format!("GitHub Actions rerun failed: {error}"),
                    auto_merge_current,
                    what_happened,
                    what_i_did,
                )
                .await;
            }
            let errors = rerun_errors
                .into_iter()
                .map(|(_, error)| error)
                .collect::<Vec<_>>()
                .join("; ");
            await_transient_ci_runs_outcome(
                repair_repo,
                reserved,
                summary,
                &hold,
                &format!("GitHub Actions rerun is temporarily unavailable: {errors}"),
                auto_merge_current,
                what_happened,
                what_i_did,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn await_transient_ci_runs_outcome(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    hold: &CiHoldIdentity,
    message: &str,
    auto_merge_current: Option<bool>,
    what_happened: Option<&str>,
    what_i_did: Option<&str>,
) -> AppResult<TransientCiRerunOutcome> {
    match reserve_agent_workspace_ci_await(
        repair_repo,
        attempt,
        &hold.to_fingerprint(),
        summary,
        auto_merge_current,
        what_happened,
        what_i_did,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(_) => {
            Ok(TransientCiRerunOutcome::RerunPending(message.to_string()))
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            Ok(TransientCiRerunOutcome::ReservationStale)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn block_transient_ci_rerun_outcome(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    blocker: &str,
    auto_merge_current: Option<bool>,
    what_happened: Option<&str>,
    what_i_did: Option<&str>,
) -> AppResult<TransientCiRerunOutcome> {
    match block_agent_workspace_repair_completion(
        repair_repo,
        branch_update_repo,
        attempt,
        summary,
        blocker,
        auto_merge_current,
        what_happened,
        what_i_did,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(_) => {
            Ok(TransientCiRerunOutcome::Blocked(blocker.to_string()))
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => Ok(TransientCiRerunOutcome::Blocked(
            "The CI rerun request became stale before it could be settled.".to_string(),
        )),
    }
}

fn github_rerun_error_is_transient(error: &AppError) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    let rate_limited = message.contains("rate limit") || message.contains("secondary rate");
    if message.contains("not found")
        || mentions_http_status(&message, "404")
        || message.contains("no such run")
        || message.contains("authentication")
        || message.contains("unauthorized")
        || mentions_http_status(&message, "401")
        || (message.contains("403 forbidden") && !rate_limited)
        || message.contains("gh auth")
        || message.contains("workflow file")
        || message.contains("cannot be rerun")
    {
        return false;
    }
    rate_limited
        || message.contains("in progress")
        || message.contains("currently in progress")
        || message.contains("timeout")
        || message.contains("timed out")
        || message.contains("connection")
        || message.contains("network")
        || message.contains("temporarily")
        || mentions_http_status(&message, "502")
        || mentions_http_status(&message, "503")
        || mentions_http_status(&message, "504")
        || message.contains("server error")
}

/// True when `code` appears as an HTTP status token rather than as a substring of an
/// unrelated number such as a workflow run id echoed in a `gh` API URL.
fn mentions_http_status(message: &str, code: &str) -> bool {
    message.match_indices(code).any(|(start, _)| {
        let bytes = message.as_bytes();
        let before_is_digit = start
            .checked_sub(1)
            .is_some_and(|index| bytes[index].is_ascii_digit());
        let after_is_digit = bytes
            .get(start + code.len())
            .is_some_and(|byte| byte.is_ascii_digit());
        !before_is_digit && !after_is_digit
    })
}

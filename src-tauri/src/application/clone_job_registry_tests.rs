use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::clone_job_registry::{CloneCancelToken, CloneJobRegistry, CloneJobStatus};
use crate::application::git_service::clone::ClonePhase;
use crate::infrastructure::git_auth::RepositoryCapability;

const RETENTION: Duration = Duration::from_secs(900);

fn destination(name: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/ralphx-clone-tests/{name}"))
}

fn completed(destination: &Path) -> CloneJobStatus {
    CloneJobStatus::Completed {
        destination: destination.display().to_string(),
        default_branch: Some("main".to_string()),
        capability: RepositoryCapability::LocalOnly,
    }
}

#[test]
fn a_new_job_starts_running_with_no_progress_yet() {
    let registry = CloneJobRegistry::new();
    let target = destination("fresh");

    let registration = registry.start("job-1".to_string(), target, RETENTION);

    assert!(!registration.deduplicated);
    assert_eq!(
        registry.status("job-1", RETENTION),
        CloneJobStatus::Running {
            phase: None,
            percent: None
        }
    );
}

/// A double-click must not start two clones fighting over one directory.
#[test]
fn starting_a_second_job_for_a_live_destination_returns_the_existing_job() {
    let registry = CloneJobRegistry::new();
    let target = destination("shared");

    let first = registry.start("job-1".to_string(), target.clone(), RETENTION);
    let second = registry.start("job-2".to_string(), target, RETENTION);

    assert!(second.deduplicated);
    assert_eq!(second.job_id, first.job_id);
    assert_eq!(registry.live_job_count(), 1);
    assert_eq!(registry.status("job-2", RETENTION), CloneJobStatus::Unknown);
}

#[test]
fn a_settled_destination_can_be_cloned_again() {
    let registry = CloneJobRegistry::new();
    let target = destination("retry");

    registry.start("job-1".to_string(), target.clone(), RETENTION);
    registry.settle("job-1", completed(&target));
    let second = registry.start("job-2".to_string(), target, RETENTION);

    assert!(
        !second.deduplicated,
        "dedupe should only apply to jobs still in flight"
    );
    assert_eq!(second.job_id, "job-2");
}

#[test]
fn progress_is_readable_by_a_late_subscriber() {
    let registry = CloneJobRegistry::new();
    registry.start("job-1".to_string(), destination("progress"), RETENTION);

    registry.record_progress("job-1", ClonePhase::Receiving, Some(42));

    assert_eq!(
        registry.status("job-1", RETENTION),
        CloneJobStatus::Running {
            phase: Some(ClonePhase::Receiving),
            percent: Some(42)
        }
    );
}

/// The whole reason the registry exists: a UI that missed every event must still
/// be able to learn how the clone ended.
#[test]
fn a_terminal_outcome_stays_readable_after_the_job_ends() {
    let registry = CloneJobRegistry::new();
    let target = destination("terminal");
    registry.start("job-1".to_string(), target.clone(), RETENTION);
    registry.record_progress("job-1", ClonePhase::Receiving, Some(90));

    registry.settle("job-1", completed(&target));

    assert_eq!(registry.status("job-1", RETENTION), completed(&target));
    assert!(registry.status("job-1", RETENTION).is_terminal());
}

#[test]
fn the_first_terminal_answer_wins() {
    let registry = CloneJobRegistry::new();
    let target = destination("first-wins");
    registry.start("job-1".to_string(), target.clone(), RETENTION);

    registry.settle("job-1", CloneJobStatus::Cancelled { cleaned_up: true });
    registry.settle("job-1", completed(&target));

    assert_eq!(
        registry.status("job-1", RETENTION),
        CloneJobStatus::Cancelled { cleaned_up: true }
    );
}

#[test]
fn progress_after_settling_is_ignored() {
    let registry = CloneJobRegistry::new();
    let target = destination("late-progress");
    registry.start("job-1".to_string(), target.clone(), RETENTION);
    registry.settle("job-1", completed(&target));

    registry.record_progress("job-1", ClonePhase::Receiving, Some(10));

    assert_eq!(registry.status("job-1", RETENTION), completed(&target));
}

#[test]
fn a_settled_job_is_pruned_once_its_retention_window_passes() {
    let registry = CloneJobRegistry::new();
    let target = destination("pruned");
    registry.start("job-1".to_string(), target.clone(), RETENTION);
    registry.settle("job-1", completed(&target));

    // Zero retention makes any settled entry immediately expired.
    assert_eq!(
        registry.status("job-1", Duration::from_secs(0)),
        CloneJobStatus::Unknown
    );
}

#[test]
fn a_running_job_is_never_pruned() {
    let registry = CloneJobRegistry::new();
    registry.start("job-1".to_string(), destination("running"), RETENTION);

    assert!(matches!(
        registry.status("job-1", Duration::from_secs(0)),
        CloneJobStatus::Running { .. }
    ));
}

#[test]
fn an_unknown_job_id_fails_closed_instead_of_looking_busy() {
    let registry = CloneJobRegistry::new();

    assert_eq!(
        registry.status("never-existed", RETENTION),
        CloneJobStatus::Unknown
    );
    assert!(registry.status("never-existed", RETENTION).is_terminal());
}

#[test]
fn cancel_takes_effect_exactly_once() {
    let registry = CloneJobRegistry::new();
    let registration = registry.start("job-1".to_string(), destination("cancel"), RETENTION);

    assert!(registry.cancel("job-1"), "the first cancel should take");
    assert!(registration.cancel.is_cancelled());
    assert!(
        !registry.cancel("job-1"),
        "a second cancel must not produce a second terminal event"
    );
}

#[test]
fn cancelling_an_unknown_or_settled_job_is_refused() {
    let registry = CloneJobRegistry::new();
    let target = destination("settled");
    registry.start("job-1".to_string(), target.clone(), RETENTION);
    registry.settle("job-1", completed(&target));

    assert!(!registry.cancel("job-1"));
    assert!(!registry.cancel("never-existed"));
}

#[test]
fn cancel_all_stops_every_live_job_and_leaves_settled_ones_alone() {
    let registry = CloneJobRegistry::new();
    let live = registry.start("job-1".to_string(), destination("a"), RETENTION);
    let settled = registry.start("job-2".to_string(), destination("b"), RETENTION);
    registry.settle("job-2", CloneJobStatus::Cancelled { cleaned_up: true });

    registry.cancel_all();

    assert!(live.cancel.is_cancelled());
    assert!(!settled.cancel.is_cancelled());
}

#[test]
fn live_destinations_are_visible_for_preflight_validation() {
    let registry = CloneJobRegistry::new();
    let target = destination("busy");
    registry.start("job-1".to_string(), target.clone(), RETENTION);

    assert!(registry.has_live_job_for(&target));
    assert!(!registry.has_live_job_for(&destination("idle")));
    assert_eq!(registry.destination_of("job-1"), Some(target));
}

// ── cancellation token ───────────────────────────────────────────────────────

#[tokio::test]
async fn a_cancel_that_arrives_before_the_wait_is_still_observed() {
    let token = Arc::new(CloneCancelToken::default());

    // The race the flag exists for: cancel first, wait afterwards.
    assert!(token.cancel());
    tokio::time::timeout(Duration::from_secs(1), Arc::clone(&token).cancelled())
        .await
        .expect("an already-cancelled token must resolve immediately");
}

#[tokio::test]
async fn a_cancel_that_arrives_during_the_wait_wakes_the_waiter() {
    let token = Arc::new(CloneCancelToken::default());
    let waiter = tokio::spawn(Arc::clone(&token).cancelled());

    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(token.cancel());

    tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("the waiter should wake")
        .expect("the waiter task should not panic");
}

#[tokio::test]
async fn an_uncancelled_token_never_resolves() {
    let token = Arc::new(CloneCancelToken::default());

    let result =
        tokio::time::timeout(Duration::from_millis(50), Arc::clone(&token).cancelled()).await;

    assert!(
        result.is_err(),
        "a live clone must not see a phantom cancel"
    );
}

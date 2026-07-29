//! Focused tests for the host observability counters (§5.5, R-11).
//!
//! The load suite (`tests/suite_remote_load`) is what proves the counters are WIRED to
//! the real teardown/prune/publish paths. These prove the tallies themselves are
//! total and correctly attributed, so a load-test assertion that reads
//! `reset_cursor_pruned` is reading the thing it names.

use ralphx_remote_protocol::{ResetReason, RESET_REASONS};

use super::counters::RemoteStreamCounters;
use super::sequencer::EpochRollCause;

#[test]
fn a_fresh_counter_set_is_all_zero() {
    let snapshot = RemoteStreamCounters::new().snapshot();

    assert_eq!(snapshot, Default::default());
    assert_eq!(snapshot.resets_total(), 0);
    assert_eq!(snapshot.epoch_rolls_total(), 0);
}

#[test]
fn prune_counts_runs_separately_from_rows() {
    let counters = RemoteStreamCounters::new();

    // A pass that deletes nothing still ran: distinguishing the two is the whole point,
    // because "the pruner is keeping up" and "the pruner has work to do" are different
    // questions and tuning retention needs both.
    counters.record_prune(0);
    counters.record_prune(120);
    counters.record_prune(3);

    let snapshot = counters.snapshot();
    assert_eq!(snapshot.prune_runs, 3);
    assert_eq!(snapshot.pruned_rows, 123);
}

/// Attribution is load-bearing for tuning: `cursor_pruned` says retention is too tight
/// for observed client dwell, `epoch_changed` says the sequencer is rolling. A `match`
/// that sent two reasons to one counter would silently merge those signals.
#[test]
fn every_reset_reason_lands_in_its_own_counter() {
    for reason in RESET_REASONS {
        let counters = RemoteStreamCounters::new();
        counters.record_reset(*reason);
        let snapshot = counters.snapshot();

        assert_eq!(
            snapshot.resets_total(),
            1,
            "{reason:?} must count exactly once"
        );
        let attributed = match reason {
            ResetReason::CursorPruned => snapshot.reset_cursor_pruned,
            ResetReason::EpochChanged => snapshot.reset_epoch_changed,
            ResetReason::AfterSeqGtMax => snapshot.reset_after_seq_gt_max,
            ResetReason::ReadError => snapshot.reset_read_error,
            ResetReason::Revoked => snapshot.reset_revoked,
            ResetReason::HostDisabled => snapshot.reset_host_disabled,
        };
        assert_eq!(attributed, 1, "{reason:?} landed in the wrong counter");
    }
}

#[test]
fn epoch_rolls_are_split_by_cause() {
    let counters = RemoteStreamCounters::new();

    counters.record_epoch_roll(EpochRollCause::CaptureOverload);
    counters.record_epoch_roll(EpochRollCause::CaptureOverload);
    counters.record_epoch_roll(EpochRollCause::CommitFailure);

    let snapshot = counters.snapshot();
    assert_eq!(snapshot.epoch_rolls_capture_overload, 2);
    assert_eq!(snapshot.epoch_rolls_commit_failure, 1);
    assert_eq!(snapshot.epoch_rolls_total(), 3);
}

/// The high-water is a running MAXIMUM, not a last-value gauge. A lower later sample
/// must not erase the peak — the peak is the only number that answers "did the queue
/// stay bounded under the storm".
#[test]
fn the_send_queue_high_water_keeps_the_peak() {
    let counters = RemoteStreamCounters::new();

    counters.observe_send_queue_depth(3);
    counters.observe_send_queue_depth(17);
    counters.observe_send_queue_depth(0);
    counters.observe_send_queue_depth(9);

    assert_eq!(counters.snapshot().send_queue_high_water, 17);
}

#[test]
fn transient_drops_and_kicks_are_independent_tallies() {
    let counters = RemoteStreamCounters::new();

    counters.record_transient_drop();
    counters.record_transient_drop();
    counters.record_kicked_session();

    let snapshot = counters.snapshot();
    assert_eq!(snapshot.transient_drops, 2);
    assert_eq!(snapshot.kicked_sessions, 1);
    // A kick is not a reset by itself: the reset is counted where the frame is sent, so
    // conflating them here would double-count every teardown.
    assert_eq!(snapshot.resets_total(), 0);
}

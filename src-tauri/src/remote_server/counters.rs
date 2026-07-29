//! Host-side observability counters for the remote event stream (§5.5, R-11).
//!
//! These exist to make the retention and heartbeat constants TUNABLE FROM MEASUREMENT
//! rather than from argument. Before this module the host reported one-shot `tracing`
//! lines and nothing aggregate, so "the send queue stays bounded" and "prune keeps up
//! under churn" were assertions with no way to falsify them.
//!
//! They are also the load tests' only honest proxy for "host memory stays flat": a Rust
//! load test has no reliable per-test RSS or allocator assertion, so the phase doc
//! (§PR 3.3, key point 3) operationalizes that claim as a bounded send-queue high-water
//! plus the drop/kick tallies recorded here.
//!
//! Every counter is monotonic for the life of a stream except `send_queue_high_water`,
//! which is a running maximum. Nothing here is persisted: the numbers describe the
//! CURRENT boot's stream, which is the only scope in which they are meaningful (the
//! epoch is per-boot by construction, §3.4).

use std::sync::atomic::{AtomicU64, Ordering};

use ralphx_remote_protocol::ResetReason;

use super::sequencer::EpochRollCause;

/// A snapshot of the counters, safe to serialize into the listener status surface.
///
/// Taken field-by-field rather than under a lock: these are independent monotonic
/// tallies read for human diagnosis, so a snapshot that catches two of them a few
/// microseconds apart is not a correctness problem. Introducing a lock on the counter
/// path to make the snapshot atomic would put a mutex in the publish hot path — exactly
/// the R-2 bottleneck this PR exists to rule out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStreamCounterSnapshot {
    /// Prune passes that ran, whether or not they deleted anything.
    pub prune_runs: u64,
    /// Durable rows actually deleted by the pruner.
    pub pruned_rows: u64,
    /// Sessions torn down with a `reset`, split by the reason the client is told.
    pub reset_cursor_pruned: u64,
    pub reset_epoch_changed: u64,
    pub reset_after_seq_gt_max: u64,
    pub reset_read_error: u64,
    pub reset_revoked: u64,
    pub reset_host_disabled: u64,
    /// In-memory epoch rolls, split by cause (§3.4 sequencer rule 3).
    pub epoch_rolls_capture_overload: u64,
    pub epoch_rolls_commit_failure: u64,
    /// Transient frames dropped by a full per-session send queue (drop-oldest, R-6).
    pub transient_drops: u64,
    /// Sessions kicked because they could not keep up with the durable stream.
    pub kicked_sessions: u64,
    /// The deepest any single session's send queue has been observed.
    pub send_queue_high_water: u64,
}

impl RemoteStreamCounterSnapshot {
    /// Total resets across every reason — the headline number for the status surface.
    pub fn resets_total(&self) -> u64 {
        self.reset_cursor_pruned
            + self.reset_epoch_changed
            + self.reset_after_seq_gt_max
            + self.reset_read_error
            + self.reset_revoked
            + self.reset_host_disabled
    }

    /// Total epoch rolls across every cause.
    pub fn epoch_rolls_total(&self) -> u64 {
        self.epoch_rolls_capture_overload + self.epoch_rolls_commit_failure
    }
}

/// The live counters, shared by every writer on one stream.
///
/// `Relaxed` ordering throughout: each counter is an independent tally with no
/// happens-before relationship to any other value, and no decision anywhere in the host
/// is made by reading one. Using `AcqRel` here would buy nothing and would put a
/// barrier in the per-frame publish path.
#[derive(Debug, Default)]
pub(crate) struct RemoteStreamCounters {
    prune_runs: AtomicU64,
    pruned_rows: AtomicU64,
    reset_cursor_pruned: AtomicU64,
    reset_epoch_changed: AtomicU64,
    reset_after_seq_gt_max: AtomicU64,
    reset_read_error: AtomicU64,
    reset_revoked: AtomicU64,
    reset_host_disabled: AtomicU64,
    epoch_rolls_capture_overload: AtomicU64,
    epoch_rolls_commit_failure: AtomicU64,
    transient_drops: AtomicU64,
    kicked_sessions: AtomicU64,
    send_queue_high_water: AtomicU64,
}

impl RemoteStreamCounters {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record_prune(&self, deleted: u64) {
        self.prune_runs.fetch_add(1, Ordering::Relaxed);
        if deleted > 0 {
            self.pruned_rows.fetch_add(deleted, Ordering::Relaxed);
        }
    }

    /// Counts one session teardown, attributed to the reason the client is told.
    ///
    /// Attribution matters for tuning: `cursor_pruned` under load means retention is
    /// too aggressive for the observed client dwell time, while `epoch_changed` means
    /// the sequencer is rolling — two different constants to move.
    pub(crate) fn record_reset(&self, reason: ResetReason) {
        let counter = match reason {
            ResetReason::CursorPruned => &self.reset_cursor_pruned,
            ResetReason::EpochChanged => &self.reset_epoch_changed,
            ResetReason::AfterSeqGtMax => &self.reset_after_seq_gt_max,
            ResetReason::ReadError => &self.reset_read_error,
            ResetReason::Revoked => &self.reset_revoked,
            ResetReason::HostDisabled => &self.reset_host_disabled,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_epoch_roll(&self, cause: EpochRollCause) {
        let counter = match cause {
            EpochRollCause::CaptureOverload => &self.epoch_rolls_capture_overload,
            EpochRollCause::CommitFailure => &self.epoch_rolls_commit_failure,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_transient_drop(&self) {
        self.transient_drops.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_kicked_session(&self) {
        self.kicked_sessions.fetch_add(1, Ordering::Relaxed);
    }

    /// Raises the high-water mark to `depth` if it is a new maximum.
    pub(crate) fn observe_send_queue_depth(&self, depth: u64) {
        self.send_queue_high_water
            .fetch_max(depth, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> RemoteStreamCounterSnapshot {
        RemoteStreamCounterSnapshot {
            prune_runs: self.prune_runs.load(Ordering::Relaxed),
            pruned_rows: self.pruned_rows.load(Ordering::Relaxed),
            reset_cursor_pruned: self.reset_cursor_pruned.load(Ordering::Relaxed),
            reset_epoch_changed: self.reset_epoch_changed.load(Ordering::Relaxed),
            reset_after_seq_gt_max: self.reset_after_seq_gt_max.load(Ordering::Relaxed),
            reset_read_error: self.reset_read_error.load(Ordering::Relaxed),
            reset_revoked: self.reset_revoked.load(Ordering::Relaxed),
            reset_host_disabled: self.reset_host_disabled.load(Ordering::Relaxed),
            epoch_rolls_capture_overload: self.epoch_rolls_capture_overload.load(Ordering::Relaxed),
            epoch_rolls_commit_failure: self.epoch_rolls_commit_failure.load(Ordering::Relaxed),
            transient_drops: self.transient_drops.load(Ordering::Relaxed),
            kicked_sessions: self.kicked_sessions.load(Ordering::Relaxed),
            send_queue_high_water: self.send_queue_high_water.load(Ordering::Relaxed),
        }
    }
}

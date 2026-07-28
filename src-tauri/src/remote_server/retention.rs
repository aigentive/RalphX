//! Retention leases — the in-memory pin that keeps `remote_event_log` rows alive while a
//! subscriber still needs them (§3.4 retention lease, R4-H3).
//!
//! **The inequality, stated once.** A lease at cursor `C` means the holder has consumed
//! everything `<= C` and still needs everything `> C`. So the pruner may delete only rows with
//! `seq <= min(live lease cursors)` and must **never** delete a row `>` any live cursor. The
//! round-3 spec text had this reversed — it protected exactly the rows nobody needs while
//! permitting deletion of the hydration delta the `H`-barrier exists to preserve. Everything in
//! this module is arranged so the safe direction is the only one expressible: the registry
//! reports a *floor* (`u64::MAX` when nothing is leased) and the pruner clamps its ceiling to it.
//!
//! **TTL is refreshed by progress, not by liveness** (§3.4 lease lifecycle). A connected client
//! that never `cursorAck`s is a dead consumer; letting a heartbeat refresh its lease would let
//! one wedged socket pin log growth forever. Only a `cursorAck` that actually moves the cursor
//! forward refreshes the lease.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;

/// Default lease TTL (§3.4: 15 minutes).
pub(crate) const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(15 * 60);

/// Rows to retain regardless of age (§3.4: `max(50_000 rows, 7 days)`).
pub(crate) const RETAIN_ROWS: u64 = 50_000;
/// Days to retain regardless of row count.
pub(crate) const RETAIN_DAYS: i64 = 7;

/// How often the background pruner runs. Retention is a housekeeping window, not a deadline.
pub(crate) const PRUNE_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Opaque lease identity. Not a session id: a session may hold at most one lease, but the
/// registry never needs to know which session, only how low the floor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct LeaseId(u64);

#[derive(Debug, Clone, Copy)]
struct LeaseEntry {
    cursor: u64,
    /// Last time the cursor actually moved forward. Liveness does not touch this.
    progressed_at: Instant,
}

#[derive(Default)]
struct LeaseTable {
    next_id: u64,
    leases: HashMap<LeaseId, LeaseEntry>,
}

/// Process-wide lease table, shared by every WS session and the pruner.
#[derive(Clone)]
pub(crate) struct RetentionLeaseRegistry {
    inner: Arc<Mutex<LeaseTable>>,
    ttl: Duration,
}

impl Default for RetentionLeaseRegistry {
    fn default() -> Self {
        Self::with_ttl(DEFAULT_LEASE_TTL)
    }
}

impl RetentionLeaseRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_ttl(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LeaseTable::default())),
            ttl,
        }
    }

    /// Acquires a lease at `cursor`.
    ///
    /// Called at `subscribe` with `cursor = afterSeq` (`= H` for the cold-hydrate barrier), so
    /// the hydration delta `> H` is prune-protected for as long as the lease lives (P-5(e)).
    pub(crate) fn acquire(&self, cursor: u64) -> RetentionLease {
        self.acquire_at(cursor, Instant::now())
    }

    pub(crate) fn acquire_at(&self, cursor: u64, now: Instant) -> RetentionLease {
        let mut table = self.lock();
        table.next_id += 1;
        let id = LeaseId(table.next_id);
        table.leases.insert(
            id,
            LeaseEntry {
                cursor,
                progressed_at: now,
            },
        );
        drop(table);
        RetentionLease {
            registry: self.clone(),
            id,
        }
    }

    /// The prune floor: `min(live lease cursors)`, or `u64::MAX` when nothing is leased.
    ///
    /// Expired leases are dropped as a side effect of the read, so a wedged consumer stops
    /// pinning the log at TTL even though its socket is still open.
    pub(crate) fn lease_floor(&self) -> u64 {
        self.lease_floor_at(Instant::now())
    }

    pub(crate) fn lease_floor_at(&self, now: Instant) -> u64 {
        self.min_live_cursor_at(now).unwrap_or(u64::MAX)
    }

    pub(crate) fn min_live_cursor_at(&self, now: Instant) -> Option<u64> {
        let mut table = self.lock();
        let ttl = self.ttl;
        table
            .leases
            .retain(|_, entry| now.duration_since(entry.progressed_at) <= ttl);
        table.leases.values().map(|entry| entry.cursor).min()
    }

    /// Live (unexpired) lease count, after evicting expired ones. Test surface: production reads
    /// the floor, never the count.
    #[cfg(test)]
    pub(crate) fn live_count_at(&self, now: Instant) -> usize {
        self.min_live_cursor_at(now);
        self.lock().leases.len()
    }

    fn advance(&self, id: LeaseId, seq: u64, now: Instant) {
        let mut table = self.lock();
        if let Some(entry) = table.leases.get_mut(&id) {
            // Only forward progress refreshes the TTL: a client re-acking the same seq forever
            // is exactly the dead consumer the TTL exists to evict.
            if seq > entry.cursor {
                entry.cursor = seq;
                entry.progressed_at = now;
            }
        }
    }

    #[cfg(test)]
    fn cursor(&self, id: LeaseId) -> Option<u64> {
        self.lock().leases.get(&id).map(|entry| entry.cursor)
    }

    fn release(&self, id: LeaseId) {
        self.lock().leases.remove(&id);
    }

    /// A poisoned lease table would otherwise turn every prune into a panic; recovering the
    /// guard keeps the pruner conservative (it still reads a real floor) instead of dead.
    fn lock(&self) -> std::sync::MutexGuard<'_, LeaseTable> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// An owned lease. Dropping it releases immediately — the connection-drop release path (P-19(b))
/// is `Drop`, not a bookkeeping call a panicking session task could skip.
pub(crate) struct RetentionLease {
    registry: RetentionLeaseRegistry,
    id: LeaseId,
}

impl RetentionLease {
    /// Test surface: production reads the registry-wide floor, never one lease's cursor.
    #[cfg(test)]
    pub(crate) fn cursor(&self) -> Option<u64> {
        self.registry.cursor(self.id)
    }

    /// Advances on `cursorAck{seq}` — the only frame that moves the floor (§3.2).
    pub(crate) fn advance(&self, seq: u64) {
        self.registry.advance(self.id, seq, Instant::now());
    }

    /// Clock-injecting form, so TTL behaviour is provable without sleeping.
    #[cfg(test)]
    pub(crate) fn advance_at(&self, seq: u64, now: Instant) {
        self.registry.advance(self.id, seq, now);
    }
}

impl Drop for RetentionLease {
    fn drop(&mut self) {
        self.registry.release(self.id);
    }
}

/// Runs the pruner on a fixed interval for the life of the process.
///
/// Spawned from an async context (rule 17). Every prune reads the lease floor when it builds the
/// request, so a lease held across the interval is honoured rather than raced.
pub(crate) fn spawn_pruner(stream: crate::remote_server::sequencer::RemoteStreamHandle) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PRUNE_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            prune_once(&stream).await;
        }
    });
}

pub(crate) async fn prune_once(stream: &crate::remote_server::sequencer::RemoteStreamHandle) {
    let window = stream.epoch_window();
    let request = crate::infrastructure::sqlite::RemotePruneRequest {
        current_epoch: window.epoch.as_str().to_string(),
        epoch_floor_seq: window.floor_seq,
        max_seq: stream.max_seq(),
        // Read last, but NOT a serialization point: the DELETE runs later on the DB thread, so a
        // subscription starting in between can take a lease at a cursor this prune is already
        // about to delete past (a new cursor is only guaranteed >= the PREVIOUS pruned floor, not
        // >= this prune's ceiling). That case stays fail-closed downstream rather than here — the
        // drain detects the hole/short read and `begin_subscription` re-validates the pruned floor
        // after draining — so the racing client gets `reset(cursor_pruned)` and cold-hydrates
        // instead of being served a spliced replay. Honouring it inside the delete transaction
        // (a floor re-read at DELETE time) is what would upgrade that reset back to a warm resume.
        lease_floor: stream.leases().lease_floor(),
        retain_rows: RETAIN_ROWS,
        retain_days: RETAIN_DAYS,
    };
    match stream.log().prune(request).await {
        Ok(outcome) => {
            if outcome.deleted > 0 {
                tracing::info!(
                    deleted = outcome.deleted,
                    pruned_floor = outcome.pruned_floor,
                    "Pruned the remote event log"
                );
            }
            // Publishing the floor is what turns a prune into a fail-closed `reset(cursor_pruned)`
            // for anyone below it, instead of a short replay.
            stream.record_pruned_floor(outcome.pruned_floor);
        }
        Err(error) => tracing::error!(%error, "Pruning the remote event log failed"),
    }
}

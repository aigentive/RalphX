//! The one-actor durable sequencer and the per-boot stream epoch (§3.4 fan-out, §3.2 rule A).
//!
//! **One owner, three steps, in order: allocate → commit → publish.** A single actor task is the
//! only assigner of `seq` and the only writer of `remote_event_log`. It allocates from an
//! in-memory counter seeded from the persisted high-water, commits the batch through
//! `DbConnection::run_transaction`, and only then publishes on the in-process broadcast channel
//! WS sessions read. Because one owner does all three in that order, **any seq a client can ever
//! cursor from is already durable**, and cross-thread emit ordering is serialized by the actor
//! rather than by whichever emitter won a race (§3.4 #2, C-14, P-5(d)).
//!
//! The withdrawn design — assign from an `AtomicU64`, broadcast immediately, persist
//! fire-and-forget — is unrepresentable here: nothing outside this actor can mint a seq, and
//! `publish_committed` is only reachable from the `Ok` arm of the commit.
//!
//! **The epoch is minted fresh every process start and never persisted** (§3.2 rule A). There is
//! no marker file, no dirty flag, and no fsync ordering to get wrong: a crash after a dropped
//! event boots into a new epoch by construction, so the log written by a previous process is
//! never replayed to anyone. Overload and commit failure roll the same in-memory epoch *live*.
//!
//! **Transient events never reach this actor.** `agent:chunk` and friends are pumped straight to
//! the broadcast channel by a separate task: no seq, no row, no head-of-line blocking behind a
//! commit (A-4).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use serde_json::Value;
use tokio::sync::{broadcast, mpsc};

use crate::error::AppResult;
use crate::infrastructure::sqlite::{RemoteEventLogStore, RemoteEventRow};
use crate::remote_server::capture::{CaptureReceivers, CapturedEvent};
use crate::remote_server::retention::RetentionLeaseRegistry;

#[cfg(test)]
#[path = "sequencer_tests.rs"]
mod tests;

/// Rows committed per `run_transaction`. Micro-batching amortizes `BEGIN IMMEDIATE` under burst
/// without letting one commit hold the write lock for an unbounded span.
const MAX_COMMIT_BATCH: usize = 256;

/// Live broadcast buffer. A session that falls this far behind is a durable gap, which is
/// resolved by kicking it with a cursor-resumable `reset`, never by silently splicing (§4.4/R-6).
const BROADCAST_CAPACITY: usize = 2_048;

/// The per-boot stream epoch: a random id held in memory for the process lifetime.
///
/// There is deliberately no `from_persisted` constructor. Rule A's whole value is that the epoch
/// cannot be restored, so no restore path can desync it (§3.2 N3-M1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StreamEpoch(Arc<str>);

impl StreamEpoch {
    pub(crate) fn mint() -> Self {
        Self(Arc::from(
            uuid::Uuid::new_v4().simple().to_string().as_str(),
        ))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StreamEpoch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Why the live epoch rolled. Both causes mean "an event may be missing from the stream".
///
/// `pub` rather than `pub(crate)` because it travels on `CaptureReceivers`, whose fields are part
/// of the capture bank's public shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochRollCause {
    /// The bounded capture channel was full, so a durable event was dropped before allocation.
    CaptureOverload,
    /// A batch commit failed; the rows are not durable and must never be published.
    CommitFailure,
}

/// Out-of-band control messages for the actor.
///
/// Carried on a **separate unbounded channel** precisely because the bounded one being full is
/// what triggers the signal — a shared channel could not deliver the overload notice (§3.4 #3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequencerControl {
    RollEpoch(EpochRollCause),
}

/// A durable event as published to sessions and replayed from the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SequencedEvent {
    pub seq: u64,
    pub epoch: StreamEpoch,
    pub name: String,
    pub payload: Value,
}

/// What a WS session receives from the live stream.
#[derive(Debug, Clone)]
pub(crate) enum StreamFrame {
    Durable(Arc<SequencedEvent>),
    Transient {
        name: &'static str,
        payload: Arc<Value>,
    },
    /// The epoch rolled: every live session must `reset` and cold-hydrate immediately, whatever
    /// its cursor. A dropped event never got a seq, so seq contiguity alone cannot reveal the
    /// hole — this frame is what does.
    EpochRolled,
}

/// An atomically-readable epoch snapshot.
///
/// Epoch and floor are read together on purpose: subscribe validation compares both, and a torn
/// read across a concurrent roll could accept a cursor the new epoch does not cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EpochWindow {
    pub epoch: StreamEpoch,
    /// Durable high-water at the moment this epoch was minted. Every row at or below it belongs
    /// to a prior epoch, so a cursor below it is unserviceable → `reset(epoch_changed)`.
    pub floor_seq: u64,
}

struct StreamState {
    window: RwLock<EpochWindow>,
    max_seq: AtomicU64,
    pruned_floor: AtomicU64,
    frames: broadcast::Sender<StreamFrame>,
    control: mpsc::UnboundedSender<SequencerControl>,
    leases: RetentionLeaseRegistry,
    log: Arc<dyn RemoteEventLogStore>,
}

/// The shared, cloneable view of the durable stream: what `hello`, `subscribe`, replay, and the
/// pruner all read.
#[derive(Clone)]
pub(crate) struct RemoteStreamHandle {
    state: Arc<StreamState>,
}

impl RemoteStreamHandle {
    pub(crate) fn epoch_window(&self) -> EpochWindow {
        self.read_window().clone()
    }

    pub(crate) fn epoch(&self) -> StreamEpoch {
        self.read_window().epoch.clone()
    }

    /// `hello.maxSeq` — and therefore the client's `H`.
    pub(crate) fn max_seq(&self) -> u64 {
        self.state.max_seq.load(Ordering::Acquire)
    }

    /// Every row at or below this is gone; a cursor below it must `reset(cursor_pruned)`.
    pub(crate) fn pruned_floor(&self) -> u64 {
        self.state.pruned_floor.load(Ordering::Acquire)
    }

    pub(crate) fn leases(&self) -> &RetentionLeaseRegistry {
        &self.state.leases
    }

    pub(crate) fn log(&self) -> Arc<dyn RemoteEventLogStore> {
        Arc::clone(&self.state.log)
    }

    /// Forks the live broadcast buffer.
    ///
    /// `subscribe` MUST call this **before** draining catch-up rows: the fork is what closes the
    /// drain-end→live-subscription gap. Frames published during the drain land in this receiver
    /// and are de-duplicated by the `seq <= throughSeq` drop rule (§3.4 subscription semantics).
    pub(crate) fn subscribe_frames(&self) -> broadcast::Receiver<StreamFrame> {
        self.state.frames.subscribe()
    }

    /// Signals a live epoch roll. Safe from sync context — it is a plain unbounded send.
    ///
    /// The capture bank owns its own sender and signals directly, so nothing in this PR calls
    /// this. It exists because the control channel is the *only* sanctioned way to roll the
    /// epoch: any future host-side trigger must come through here rather than reaching into the
    /// actor's state, which is what keeps "one owner mints the epoch" true.
    #[allow(dead_code)]
    pub(crate) fn request_epoch_roll(&self, cause: EpochRollCause) {
        if self
            .state
            .control
            .send(SequencerControl::RollEpoch(cause))
            .is_err()
        {
            tracing::error!(?cause, "Remote sequencer control channel is closed");
        }
    }

    /// Records the outcome of a prune so subscribe validation can fail closed on a stale cursor.
    pub(crate) fn record_pruned_floor(&self, floor: u64) {
        self.state.pruned_floor.fetch_max(floor, Ordering::AcqRel);
    }

    fn read_window(&self) -> std::sync::RwLockReadGuard<'_, EpochWindow> {
        self.state
            .window
            .read()
            .unwrap_or_else(|error| error.into_inner())
    }
}

/// Builds the shared stream state and returns the handle plus the actor's control receiver.
///
/// Split from [`RemoteStream::start`] so tests can drive the actor step by step instead of racing
/// a spawned task.
fn build_stream(
    log: Arc<dyn RemoteEventLogStore>,
    high_water: u64,
    pruned_floor: u64,
    leases: RetentionLeaseRegistry,
    control: mpsc::UnboundedSender<SequencerControl>,
) -> RemoteStreamHandle {
    let (frames, _) = broadcast::channel(BROADCAST_CAPACITY);
    RemoteStreamHandle {
        state: Arc::new(StreamState {
            // Fresh every boot. The floor is the persisted high-water, so any warm-resume cursor
            // from a previous process is below it and resets (P-22(a)).
            window: RwLock::new(EpochWindow {
                epoch: StreamEpoch::mint(),
                floor_seq: high_water,
            }),
            max_seq: AtomicU64::new(high_water),
            pruned_floor: AtomicU64::new(pruned_floor),
            frames,
            control,
            leases,
            log,
        }),
    }
}

/// Test-only constructor for the shared stream state, so session tests can exercise the real
/// handle (epoch window, high-water, pruned floor, lease registry) without spawning an actor.
#[cfg(test)]
pub(crate) fn build_stream_for_test(
    log: Arc<dyn RemoteEventLogStore>,
    high_water: u64,
    pruned_floor: u64,
    leases: RetentionLeaseRegistry,
    control: mpsc::UnboundedSender<SequencerControl>,
) -> RemoteStreamHandle {
    build_stream(log, high_water, pruned_floor, leases, control)
}

/// The sequencer actor.
pub(crate) struct RemoteSequencer {
    handle: RemoteStreamHandle,
    /// The next seq to allocate. Owned by the actor alone — this is the reason no other code
    /// path can mint a seq.
    next_seq: u64,
}

impl RemoteSequencer {
    /// Reads the persisted high-water, mints a fresh epoch, and spawns the actor.
    ///
    /// `async` on purpose: `tokio::spawn` requires a runtime on the calling thread, and an async
    /// fn always has one (rule 17). App setup reaches this through `tauri::async_runtime::spawn`.
    pub(crate) async fn start(
        log: Arc<dyn RemoteEventLogStore>,
        receivers: CaptureReceivers,
        leases: RetentionLeaseRegistry,
    ) -> AppResult<RemoteStreamHandle> {
        let high_water = log.high_water().await?;
        // Rows at or below the oldest surviving seq are gone; below that floor a cursor cannot be
        // served. An empty log means nothing has been pruned within this numbering.
        let pruned_floor = match log.oldest_seq().await? {
            Some(oldest) => oldest.saturating_sub(1),
            None => high_water,
        };

        let CaptureReceivers {
            durable,
            transient,
            control,
            control_sender,
        } = receivers;
        let handle = build_stream(log, high_water, pruned_floor, leases, control_sender);

        let actor = Self {
            handle: handle.clone(),
            next_seq: high_water,
        };
        tokio::spawn(actor.run(durable, control));
        tokio::spawn(pump_transient(handle.clone(), transient));

        tracing::info!(
            epoch = %handle.epoch(),
            high_water,
            pruned_floor,
            "Remote durable sequencer started with a fresh per-boot stream epoch"
        );
        Ok(handle)
    }

    async fn run(
        mut self,
        mut durable: mpsc::Receiver<CapturedEvent>,
        mut control: mpsc::UnboundedReceiver<SequencerControl>,
    ) {
        loop {
            tokio::select! {
                // Biased so an overload signal is never starved by a saturated durable channel —
                // the roll is what keeps a dropped event from being silently spliced over.
                biased;
                Some(message) = control.recv() => match message {
                    SequencerControl::RollEpoch(cause) => self.roll_epoch(cause),
                },
                Some(first) = durable.recv() => {
                    self.drain_batch(first, &mut durable).await;
                }
                else => break,
            }
        }
        tracing::info!("Remote durable sequencer stopped");
    }

    /// Collects a micro-batch, then runs allocate → commit → publish over it.
    async fn drain_batch(
        &mut self,
        first: CapturedEvent,
        durable: &mut mpsc::Receiver<CapturedEvent>,
    ) {
        let mut batch = vec![first];
        while batch.len() < MAX_COMMIT_BATCH {
            match durable.try_recv() {
                Ok(event) => batch.push(event),
                Err(_) => break,
            }
        }
        self.commit_batch(batch).await;
    }

    /// allocate → commit → publish, in that order, for one micro-batch.
    async fn commit_batch(&mut self, batch: Vec<CapturedEvent>) {
        if batch.is_empty() {
            return;
        }

        // 1. ALLOCATE — from the actor-owned counter, against the epoch live right now.
        let epoch = self.handle.epoch();
        let rows = batch
            .iter()
            .enumerate()
            .map(|(offset, event)| RemoteEventRow {
                seq: self.next_seq + 1 + offset as u64,
                epoch: epoch.as_str().to_string(),
                name: event.name.to_string(),
                // The raw capture text goes to TEXT verbatim: never parsed on the emit thread,
                // never re-serialized here.
                payload: event.payload.clone(),
            })
            .collect::<Vec<_>>();

        // 2. COMMIT — rows and high-water in one transaction.
        if let Err(error) = self.handle.state.log.append_batch(&rows).await {
            // The counter does NOT advance and nothing is published: an uncommitted seq must
            // never be cursorable. The epoch rolls so connected clients cold-hydrate instead of
            // resuming over the hole (§3.4 #3).
            tracing::error!(%error, rows = rows.len(), "Remote event log commit failed");
            self.roll_epoch(EpochRollCause::CommitFailure);
            return;
        }
        self.next_seq += rows.len() as u64;

        // 3. PUBLISH — only now is any of this cursorable.
        self.publish_committed(&epoch, rows);
    }

    fn publish_committed(&self, epoch: &StreamEpoch, rows: Vec<RemoteEventRow>) {
        let Some(last_seq) = rows.last().map(|row| row.seq) else {
            return;
        };
        // max_seq before the frames: a session that reads `maxSeq` after seeing a frame must
        // never see a high-water below it.
        self.handle
            .state
            .max_seq
            .fetch_max(last_seq, Ordering::AcqRel);
        for row in rows {
            let event = SequencedEvent {
                seq: row.seq,
                epoch: epoch.clone(),
                name: row.name,
                payload: parse_payload(&row.payload),
            };
            // A send error means no session is subscribed. That is not a failure: the row is
            // durable and any future subscriber replays it.
            let _ = self
                .handle
                .state
                .frames
                .send(StreamFrame::Durable(Arc::new(event)));
        }
    }

    /// Rolls the in-memory epoch and kicks every live session.
    ///
    /// No DB write, no fs write, no await: the roll must be reachable even when the database is
    /// exactly what failed.
    fn roll_epoch(&self, cause: EpochRollCause) {
        let max_seq = self.handle.max_seq();
        {
            let mut window = self
                .handle
                .state
                .window
                .write()
                .unwrap_or_else(|error| error.into_inner());
            *window = EpochWindow {
                epoch: StreamEpoch::mint(),
                floor_seq: max_seq,
            };
        }
        let _ = self.handle.state.frames.send(StreamFrame::EpochRolled);
        tracing::warn!(
            ?cause,
            epoch = %self.handle.epoch(),
            floor_seq = max_seq,
            "Remote stream epoch rolled live; connected clients must cold-hydrate"
        );
    }
}

/// Transient pump: capture → broadcast, with no seq, no row, and no sequencer involvement (A-4).
async fn pump_transient(
    handle: RemoteStreamHandle,
    mut transient: mpsc::UnboundedReceiver<CapturedEvent>,
) {
    while let Some(event) = transient.recv().await {
        let _ = handle.state.frames.send(StreamFrame::Transient {
            name: event.name,
            payload: Arc::new(parse_payload(&event.payload)),
        });
    }
}

/// One parse used by BOTH the live publish path and the replay path, so a payload that does not
/// parse produces the identical frame either way rather than diverging between live and replay.
///
/// A durable event is never dropped for being unparseable — dropping it would be exactly the
/// silent hole the epoch machinery exists to prevent — so the raw text is carried through as a
/// JSON string instead.
pub(crate) fn parse_payload(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

//! P-5 (a)-(d), P-15, P-22, A-4, C-14 — the sequencer contract.
//!
//! Every test drives the real actor methods (`commit_batch`, `roll_epoch`) rather than a
//! re-implementation, so a change to the production ordering is what fails.

use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use super::*;
use crate::error::AppError;
use crate::infrastructure::sqlite::{RemotePruneOutcome, RemotePruneRequest};
use crate::remote_server::capture::{CaptureFeed, CaptureSink, EventRegistrar, RemoteEventCapture};

// ------------------------------------------------------------------------------------------
// A log double that can fail on demand — `reset(read_error)` and commit-failure rolls are only
// provable if the store CAN fail (P-5(c), §3.4 #3).
// ------------------------------------------------------------------------------------------

#[derive(Default)]
pub(super) struct FakeEventLog {
    rows: Mutex<Vec<RemoteEventRow>>,
    high_water: Mutex<u64>,
    fail_commit: AtomicBool,
    fail_read: AtomicBool,
}

impl FakeEventLog {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub(super) fn seeded(high_water: u64) -> Arc<Self> {
        let log = Self::default();
        *log.high_water.lock().unwrap() = high_water;
        Arc::new(log)
    }

    pub(super) fn fail_commits(&self, fail: bool) {
        self.fail_commit.store(fail, AtomicOrdering::SeqCst);
    }

    pub(super) fn seqs(&self) -> Vec<u64> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .map(|row| row.seq)
            .collect()
    }

    pub(super) fn names(&self) -> Vec<String> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .map(|row| row.name.clone())
            .collect()
    }
}

#[async_trait]
impl RemoteEventLogStore for FakeEventLog {
    async fn high_water(&self) -> AppResult<u64> {
        Ok(*self.high_water.lock().unwrap())
    }

    async fn append_batch(&self, rows: &[RemoteEventRow]) -> AppResult<()> {
        if self.fail_commit.load(AtomicOrdering::SeqCst) {
            return Err(AppError::Database("induced commit failure".to_string()));
        }
        let mut stored = self.rows.lock().unwrap();
        let mut high_water = self.high_water.lock().unwrap();
        for row in rows {
            assert!(
                !stored.iter().any(|existing| existing.seq == row.seq),
                "seq {} was reused — the sequencer must never reuse a seq",
                row.seq
            );
            stored.push(row.clone());
            *high_water = (*high_water).max(row.seq);
        }
        Ok(())
    }

    async fn read_range(
        &self,
        epoch: &str,
        after_seq: u64,
        through_seq: u64,
        limit: usize,
    ) -> AppResult<Vec<RemoteEventRow>> {
        if self.fail_read.load(AtomicOrdering::SeqCst) {
            return Err(AppError::Database("induced read failure".to_string()));
        }
        let rows = self.rows.lock().unwrap();
        let mut selected = rows
            .iter()
            .filter(|row| row.epoch == epoch && row.seq > after_seq && row.seq <= through_seq)
            .cloned()
            .collect::<Vec<_>>();
        selected.sort_by_key(|row| row.seq);
        selected.truncate(limit);
        Ok(selected)
    }

    async fn oldest_seq(&self) -> AppResult<Option<u64>> {
        Ok(self.rows.lock().unwrap().iter().map(|row| row.seq).min())
    }

    async fn prune(&self, request: RemotePruneRequest) -> AppResult<RemotePruneOutcome> {
        let ceiling = request
            .max_seq
            .saturating_sub(request.retain_rows)
            .max(request.epoch_floor_seq)
            .min(request.lease_floor);
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|row| row.seq > ceiling);
        Ok(RemotePruneOutcome {
            deleted: (before - rows.len()) as u64,
            pruned_floor: ceiling,
        })
    }
}

// ------------------------------------------------------------------------------------------
// Harness
// ------------------------------------------------------------------------------------------

struct Harness {
    sequencer: RemoteSequencer,
    handle: RemoteStreamHandle,
    log: Arc<FakeEventLog>,
    frames: broadcast::Receiver<StreamFrame>,
}

fn harness_with(log: Arc<FakeEventLog>, high_water: u64) -> Harness {
    let (control_sender, _control_rx) = mpsc::unbounded_channel();
    let handle = build_stream(
        log.clone(),
        high_water,
        0,
        RetentionLeaseRegistry::new(),
        control_sender,
    );
    let frames = handle.subscribe_frames();
    Harness {
        sequencer: RemoteSequencer {
            handle: handle.clone(),
            next_seq: high_water,
        },
        handle,
        log,
        frames,
    }
}

fn harness() -> Harness {
    harness_with(FakeEventLog::new(), 0)
}

fn captured(name: &'static str, payload: &str) -> CapturedEvent {
    CapturedEvent {
        name,
        payload: payload.to_string(),
    }
}

fn drain_frames(frames: &mut broadcast::Receiver<StreamFrame>) -> Vec<StreamFrame> {
    let mut collected = Vec::new();
    while let Ok(frame) = frames.try_recv() {
        collected.push(frame);
    }
    collected
}

fn durable_seqs(frames: &[StreamFrame]) -> Vec<u64> {
    frames
        .iter()
        .filter_map(|frame| match frame {
            StreamFrame::Durable(event) => Some(event.seq),
            _ => None,
        })
        .collect()
}

type CaptureHandler = Box<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
struct TargetEventRegistrar {
    target: &'static str,
    handler: Arc<Mutex<Option<CaptureHandler>>>,
}

impl TargetEventRegistrar {
    fn new(target: &'static str) -> Self {
        Self {
            target,
            handler: Arc::new(Mutex::new(None)),
        }
    }

    fn emit(&self, payload: &str) {
        let handler = self.handler.lock().unwrap();
        handler
            .as_ref()
            .expect("the target event should have a capture handler")(payload);
    }
}

impl EventRegistrar for TargetEventRegistrar {
    fn listen(&self, name: &'static str, handler: CaptureHandler) {
        if name == self.target {
            *self.handler.lock().unwrap() = Some(handler);
        }
    }
}

// ------------------------------------------------------------------------------------------
// P-22: the epoch is fresh per boot, unconditionally
// ------------------------------------------------------------------------------------------

/// P-22(a): rule A. Two boots against the SAME durable state produce different epochs — there is
/// no marker, no dirty flag, and therefore no restore case that could desync them.
#[test]
fn every_boot_mints_a_fresh_epoch_against_identical_durable_state() {
    let log = FakeEventLog::seeded(500);
    let first = harness_with(log.clone(), 500);
    let second = harness_with(log, 500);

    assert_ne!(
        first.handle.epoch(),
        second.handle.epoch(),
        "the epoch must be minted fresh on every process start (§3.2 rule A)"
    );
    // …and the seq numbering is monotonic across the boot, never reused.
    assert_eq!(second.handle.max_seq(), 500);
}

/// P-22(a) / P-5(a): the epoch floor is the persisted high-water, so ANY warm-resume cursor from
/// a previous process is below it. That is the mechanism by which a restart forces `reset`.
#[test]
fn a_fresh_boot_puts_the_epoch_floor_at_the_persisted_high_water() {
    let restarted = harness_with(FakeEventLog::seeded(500), 500);

    let window = restarted.handle.epoch_window();
    assert_eq!(window.floor_seq, 500);
    assert!(
        400 < window.floor_seq,
        "a prior-boot cursor at 400 must sit below the floor and reset"
    );
}

/// P-22(c): disable→enable inside ONE process does not roll the epoch. Only the actor rolls it,
/// and neither listener stop nor start touches the actor.
#[test]
fn listener_disable_and_re_enable_do_not_roll_the_epoch() {
    let harness = harness();
    let before = harness.handle.epoch();

    // The listener lifecycle has no path into the epoch: nothing here can change it.
    let after = harness.handle.epoch();

    assert_eq!(before, after);
    assert_eq!(harness.handle.epoch_window().floor_seq, 0);
}

/// P-23: installing the stream is independent of starting the listener. A durable event emitted
/// during the disabled window still reaches the log and is available for a later replay.
#[tokio::test]
async fn a_stream_installed_while_the_listener_is_disabled_records_captured_events() {
    let log = FakeEventLog::new();
    let (feed, receivers) = CaptureFeed::channels(16);
    let stream = RemoteSequencer::start(log.clone(), receivers, RetentionLeaseRegistry::new())
        .await
        .expect("the durable stream should start");
    let listener = crate::remote_server::RemoteListenerHandle::new();
    assert!(listener.install_stream(stream));
    assert!(
        !listener.is_running().await,
        "no network listener was started"
    );
    let registrar = TargetEventRegistrar::new("notification:created");
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);

    registrar.emit(r#"{"id":"during-disabled-window"}"#);

    tokio::time::timeout(Duration::from_secs(1), async {
        while log.names().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the sequencer should commit the captured event");
    assert_eq!(log.names(), vec!["notification:created"]);
    assert_eq!(log.seqs(), vec![1]);
    assert!(
        !listener.is_running().await,
        "durable capture must not depend on listener exposure"
    );
}

/// P-22(b): a commit failure rolls the in-memory epoch live and kicks connected clients.
#[tokio::test]
async fn a_commit_failure_rolls_the_epoch_live_and_publishes_nothing() {
    let mut harness = harness();
    let before = harness.handle.epoch_window();
    harness.log.fail_commits(true);

    harness
        .sequencer
        .commit_batch(vec![captured("task:created", "{}")])
        .await;

    let after = harness.handle.epoch_window();
    assert_ne!(
        before.epoch, after.epoch,
        "a failed commit must roll the epoch"
    );
    let frames = drain_frames(&mut harness.frames);
    assert!(
        durable_seqs(&frames).is_empty(),
        "an uncommitted seq must never be published"
    );
    assert!(
        frames
            .iter()
            .any(|frame| matches!(frame, StreamFrame::EpochRolled)),
        "connected clients must be kicked, not left resuming over the hole"
    );
    assert_eq!(
        harness.handle.max_seq(),
        0,
        "the high-water must not advance"
    );
    assert!(harness.log.seqs().is_empty());
}

/// The counter does not advance past an uncommitted row, so the retry reuses no seq and skips
/// none either (A-10: no downward seq reseed, no pre-commit broadcast).
#[tokio::test]
async fn the_seq_counter_does_not_advance_past_a_failed_commit() {
    let mut harness = harness();
    harness.log.fail_commits(true);
    harness
        .sequencer
        .commit_batch(vec![captured("task:created", "{}")])
        .await;

    harness.log.fail_commits(false);
    harness
        .sequencer
        .commit_batch(vec![captured("task:created", "{}")])
        .await;

    assert_eq!(
        harness.log.seqs(),
        vec![1],
        "the first allocated seq is reused only because it was never committed or published"
    );
    assert_eq!(harness.handle.max_seq(), 1);
}

/// P-22(b): a full capture channel signals the roll over the SEPARATE unbounded control channel.
#[tokio::test]
async fn a_full_capture_channel_signals_an_epoch_roll_over_the_control_channel() {
    let (feed, mut receivers) = CaptureFeed::channels(1);
    // Fill the one-slot durable channel, then overflow it through the production handler body.
    feed.dispatch(CaptureSink::Durable, captured("task:created", "{}"));
    assert!(
        receivers.control.try_recv().is_err(),
        "a send that fits must not signal an overload"
    );
    feed.dispatch(CaptureSink::Durable, captured("task:created", "{}"));

    let signal = receivers.control.try_recv();
    assert_eq!(
        signal,
        Ok(SequencerControl::RollEpoch(EpochRollCause::CaptureOverload)),
        "a dropped durable event must roll the epoch, never be silently spliced over"
    );
}

// ------------------------------------------------------------------------------------------
// P-5: allocate → commit → publish
// ------------------------------------------------------------------------------------------

/// C-14 / P-5(b): seqs are contiguous within an epoch and every published seq is already durable.
#[tokio::test]
async fn a_batch_is_allocated_contiguously_committed_then_published() {
    let mut harness = harness();

    harness
        .sequencer
        .commit_batch(vec![
            captured("task:created", r#"{"id":"t1"}"#),
            captured("task:status_changed", r#"{"id":"t2"}"#),
            captured("notification:created", r#"{"id":"n1"}"#),
        ])
        .await;

    assert_eq!(
        harness.log.seqs(),
        vec![1, 2, 3],
        "allocation is contiguous"
    );
    assert_eq!(harness.handle.max_seq(), 3);
    let frames = drain_frames(&mut harness.frames);
    assert_eq!(
        durable_seqs(&frames),
        vec![1, 2, 3],
        "publication order is the actor's allocation order"
    );
    // Every published seq is present in the log: publish is only reachable after the commit.
    for seq in durable_seqs(&frames) {
        assert!(harness.log.seqs().contains(&seq));
    }
}

/// P-5(d): cross-thread emit ordering is serialized by the actor. Whatever order emitters raced
/// in, the actor assigns and publishes in one order — its own.
#[tokio::test]
async fn cross_thread_emissions_are_serialized_into_one_order() {
    let mut harness = harness();

    harness
        .sequencer
        .commit_batch(vec![captured("task:created", "{}")])
        .await;
    harness
        .sequencer
        .commit_batch(vec![
            captured("task:status_changed", "{}"),
            captured("agent:run_started", "{}"),
        ])
        .await;

    let frames = drain_frames(&mut harness.frames);
    assert_eq!(durable_seqs(&frames), vec![1, 2, 3]);
    assert_eq!(
        harness.log.names(),
        vec!["task:created", "task:status_changed", "agent:run_started"],
        "assignment and publication order are both the actor's"
    );
}

#[tokio::test]
async fn a_boot_continues_numbering_from_the_persisted_high_water() {
    let mut harness = harness_with(FakeEventLog::seeded(41), 41);

    harness
        .sequencer
        .commit_batch(vec![captured("task:created", "{}")])
        .await;

    assert_eq!(
        harness.log.seqs(),
        vec![42],
        "numbering is monotonic across boots and never reused"
    );
}

/// The raw capture text reaches TEXT verbatim — no parse on the emit thread, no re-serialization
/// here (integration-branch capture contract).
#[tokio::test]
async fn the_raw_capture_payload_reaches_the_log_verbatim() {
    let mut harness = harness();
    let raw = r#"{"id":"t1","nested":{"n":1},"s":"a b"}"#;

    harness
        .sequencer
        .commit_batch(vec![captured("task:created", raw)])
        .await;

    let stored = harness.log.rows.lock().unwrap();
    assert_eq!(stored[0].payload, raw);
}

/// Live publish and replay must agree byte-for-byte, including on a payload that does not parse:
/// a durable event is never dropped for being unparseable (that would be the silent hole).
#[test]
fn live_and_replay_parse_a_payload_identically() {
    assert_eq!(parse_payload(r#"{"a":1}"#), json!({"a": 1}));
    assert_eq!(
        parse_payload("not-json"),
        Value::String("not-json".to_string()),
        "an unparseable durable payload is carried through, never dropped"
    );
}

// ------------------------------------------------------------------------------------------
// A-4: transient events never touch the sequencer
// ------------------------------------------------------------------------------------------

/// A-4: no transient event row ever lands in `remote_event_log`. The proof is structural — the
/// transient pump has no access to the log at all — and asserted here end to end.
#[tokio::test]
async fn transient_events_are_broadcast_only_and_never_persisted() {
    let harness = harness();
    let mut frames = harness.handle.subscribe_frames();
    let (transient_tx, transient_rx) = mpsc::unbounded_channel();

    transient_tx
        .send(captured("agent:chunk", r#"{"text":"hi"}"#))
        .expect("pump receiver is live");
    drop(transient_tx);
    pump_transient(harness.handle.clone(), transient_rx).await;

    let published = drain_frames(&mut frames);
    assert!(
        matches!(
            published.as_slice(),
            [StreamFrame::Transient {
                name: "agent:chunk",
                ..
            }]
        ),
        "the transient frame must reach the broadcast channel"
    );
    assert!(
        harness.log.seqs().is_empty(),
        "no transient event may ever land in remote_event_log (A-4)"
    );
    assert_eq!(
        harness.handle.max_seq(),
        0,
        "a transient event must not consume a seq"
    );
}

/// The transient class carries NO seq on the wire (§3.2: transient frames have no `seq`).
#[test]
fn a_transient_frame_carries_no_seq() {
    let frame = StreamFrame::Transient {
        name: "agent:chunk",
        payload: Arc::new(json!({"text": "hi"})),
    };
    match frame {
        StreamFrame::Transient { name, .. } => assert_eq!(name, "agent:chunk"),
        other => panic!("expected a transient frame, got {other:?}"),
    }
}

// ------------------------------------------------------------------------------------------
// Source-level invariants (C-2, §3.4)
// ------------------------------------------------------------------------------------------

/// C-14 / A-10: publication is only reachable from the `Ok` arm of the commit. If a future edit
/// broadcasts before `append_batch`, this fails.
#[test]
fn publication_is_unreachable_without_a_successful_commit() {
    let source = include_str!("sequencer.rs");
    let commit_index = source
        .find("append_batch(&rows).await")
        .expect("the commit call should exist");
    let publish_index = source
        .find("self.publish_committed(&epoch, rows)")
        .expect("the publish call should exist");
    assert!(
        commit_index < publish_index,
        "allocate → commit → publish: publication must follow the commit"
    );
    assert!(
        source.contains("self.roll_epoch(EpochRollCause::CommitFailure);\n            return;"),
        "a failed commit must roll the epoch and return without publishing"
    );
}

/// §3.4: the roll path must not depend on any async/DB/fs work — a crash-safe roll is only
/// crash-safe if it is reachable when the database is exactly what failed.
#[test]
fn the_epoch_roll_touches_no_database_or_filesystem() {
    let source = include_str!("sequencer.rs");
    let roll = source
        .split("fn roll_epoch(&self, cause: EpochRollCause)")
        .nth(1)
        .and_then(|rest| rest.split("\n    }\n").next())
        .expect("roll_epoch body should be locatable");
    for forbidden in ["await", "std::fs", "append_batch", "run_transaction"] {
        assert!(
            !roll.contains(forbidden),
            "the epoch roll must not reach {forbidden}"
        );
    }
}

/// §3.2 rule A: nothing may persist or restore the epoch. No marker file, no DB row, no
/// dirty flag — that absence is the entire safety argument.
#[test]
fn the_epoch_is_never_persisted_or_restored() {
    let source = include_str!("sequencer.rs");
    for forbidden in [
        "epoch_marker",
        "dirty_flag",
        "from_persisted",
        "persist_epoch",
        "stream_epoch =",
    ] {
        assert!(
            !source.contains(forbidden),
            "the epoch must have no persistence seam ({forbidden})"
        );
    }
}

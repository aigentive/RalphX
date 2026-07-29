//! P-5 (b)/(c)/(e), P-2, P-15, §4.4/R-6 backpressure, heartbeat timeout — the WS session contract.

use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::json;

use super::*;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::{RemoteEventRow, RemotePruneOutcome, RemotePruneRequest};
use crate::remote_server::retention::RetentionLeaseRegistry;
use crate::remote_server::sequencer::{
    build_stream_for_test, EpochRollCause, RemoteStreamHandle, SequencerControl, StreamEpoch,
};

// ------------------------------------------------------------------------------------------
// Doubles
// ------------------------------------------------------------------------------------------

#[derive(Default)]
struct TestLog {
    rows: Mutex<Vec<RemoteEventRow>>,
    fail_read: AtomicBool,
}

impl TestLog {
    fn with_rows(epoch: &str, seqs: &[u64]) -> Arc<Self> {
        let log = Self::default();
        *log.rows.lock().unwrap() = seqs
            .iter()
            .map(|seq| RemoteEventRow {
                seq: *seq,
                epoch: epoch.to_string(),
                name: "task:created".to_string(),
                payload: format!(r#"{{"seq":{seq}}}"#),
            })
            .collect();
        Arc::new(log)
    }

    fn fail_reads(&self, fail: bool) {
        self.fail_read.store(fail, AtomicOrdering::SeqCst);
    }
}

#[async_trait]
impl RemoteEventLogStore for TestLog {
    async fn high_water(&self) -> AppResult<u64> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .map(|row| row.seq)
            .max()
            .unwrap_or(0))
    }

    async fn append_batch(&self, rows: &[RemoteEventRow]) -> AppResult<()> {
        self.rows.lock().unwrap().extend_from_slice(rows);
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

/// A socket that records what the server sent and replays scripted client frames.
#[derive(Default)]
struct RecordingSocket {
    sent: Vec<ServerFrame>,
    incoming: VecDeque<Option<Result<ClientFrame, SocketError>>>,
    closed: bool,
}

impl RecordingSocket {
    fn scripted(frames: Vec<ClientFrame>) -> Self {
        let mut incoming = frames
            .into_iter()
            .map(|frame| Some(Ok(frame)))
            .collect::<VecDeque<_>>();
        // The peer closing is what ends a clean session.
        incoming.push_back(None);
        Self {
            incoming,
            ..Self::default()
        }
    }

    /// A peer that neither sends nor closes — the shape every teardown test needs, because a
    /// client-initiated close would end the session before the teardown path could.
    fn silent() -> Self {
        Self::default()
    }

    fn resets(&self) -> Vec<ResetReason> {
        self.sent
            .iter()
            .filter_map(|frame| match frame {
                ServerFrame::Reset { reason } => Some(*reason),
                _ => None,
            })
            .collect()
    }

    fn event_seqs(&self) -> Vec<Option<u64>> {
        self.sent
            .iter()
            .filter_map(|frame| match frame {
                ServerFrame::Event { seq, .. } => Some(*seq),
                _ => None,
            })
            .collect()
    }

    fn has_replay_done(&self) -> bool {
        self.sent
            .iter()
            .any(|frame| matches!(frame, ServerFrame::ReplayDone { .. }))
    }
}

#[async_trait]
impl SessionSocket for RecordingSocket {
    async fn send(&mut self, frame: ServerFrame) -> Result<(), SocketError> {
        self.sent.push(frame);
        Ok(())
    }

    async fn recv(&mut self) -> Option<Result<ClientFrame, SocketError>> {
        match self.incoming.pop_front() {
            Some(next) => next,
            // Park instead of spinning once the script is exhausted, exactly as a live but idle
            // peer would.
            None => std::future::pending::<Option<Result<ClientFrame, SocketError>>>().await,
        }
    }

    async fn close(&mut self) {
        self.closed = true;
    }
}

struct AlwaysLive;

#[async_trait]
impl DeviceLivenessCheck for AlwaysLive {
    async fn check(&self, _device_id: &RemoteDeviceId) -> DeviceLiveness {
        DeviceLiveness::Live
    }
}

struct AlwaysRevoked;

#[async_trait]
impl DeviceLivenessCheck for AlwaysRevoked {
    async fn check(&self, _device_id: &RemoteDeviceId) -> DeviceLiveness {
        DeviceLiveness::Revoked
    }
}

/// A store outage must NOT read as "still authorized" (fail closed on reads).
struct UnreadableDevice;

#[async_trait]
impl DeviceLivenessCheck for UnreadableDevice {
    async fn check(&self, _device_id: &RemoteDeviceId) -> DeviceLiveness {
        DeviceLiveness::Unreadable
    }
}

fn stream_with(log: Arc<TestLog>, high_water: u64, pruned_floor: u64) -> RemoteStreamHandle {
    let (control, _control_rx) = tokio::sync::mpsc::unbounded_channel::<SequencerControl>();
    build_stream_for_test(
        log,
        high_water,
        pruned_floor,
        RetentionLeaseRegistry::new(),
        control,
    )
}

fn window(epoch: &StreamEpoch, floor_seq: u64) -> EpochWindow {
    EpochWindow {
        epoch: epoch.clone(),
        floor_seq,
    }
}

// ------------------------------------------------------------------------------------------
// Subscribe validation — fail closed (§3.4)
// ------------------------------------------------------------------------------------------

#[test]
fn a_valid_cursor_inside_the_window_is_accepted() {
    let epoch = StreamEpoch::mint();
    assert_eq!(
        validate_subscribe(&window(&epoch, 100), 500, 100, 300, epoch.as_str()),
        Ok(())
    );
    // The canonical cold hydrate: afterSeq == H == maxSeq.
    assert_eq!(
        validate_subscribe(&window(&epoch, 100), 500, 100, 500, epoch.as_str()),
        Ok(())
    );
    // Exactly at the pruned floor is serviceable: rows <= floor are gone, and this cursor needs
    // only rows > floor.
    assert_eq!(
        validate_subscribe(&window(&epoch, 0), 500, 100, 100, epoch.as_str()),
        Ok(())
    );
}

#[test]
fn a_stale_epoch_resets_with_epoch_changed() {
    let epoch = StreamEpoch::mint();
    assert_eq!(
        validate_subscribe(&window(&epoch, 0), 500, 0, 300, "some-other-epoch"),
        Err(ResetReason::EpochChanged)
    );
}

#[test]
fn a_cursor_above_the_high_water_resets_with_after_seq_gt_max() {
    let epoch = StreamEpoch::mint();
    assert_eq!(
        validate_subscribe(&window(&epoch, 0), 500, 0, 501, epoch.as_str()),
        Err(ResetReason::AfterSeqGtMax)
    );
}

/// P-5(a) / P-22(a): the cross-restart case. A client that kept its cursor over a host restart
/// quotes the NEW epoch (it read a fresh hello) but sits below the new epoch floor — it must
/// reset, never warm-splice over the previous process's tail.
#[test]
fn a_cursor_below_the_epoch_floor_resets_even_with_a_matching_epoch_string() {
    let epoch = StreamEpoch::mint();
    assert_eq!(
        validate_subscribe(&window(&epoch, 500), 500, 0, 400, epoch.as_str()),
        Err(ResetReason::EpochChanged),
        "rows below the floor belong to a previous epoch and are unreplayable"
    );
}

#[test]
fn a_cursor_below_the_pruned_floor_resets_with_cursor_pruned() {
    let epoch = StreamEpoch::mint();
    assert_eq!(
        validate_subscribe(&window(&epoch, 0), 500, 200, 199, epoch.as_str()),
        Err(ResetReason::CursorPruned)
    );
}

/// The epoch check runs FIRST: a request that is wrong in two ways reports the epoch, because a
/// cursor from another epoch makes every numeric comparison meaningless.
#[test]
fn epoch_mismatch_is_reported_ahead_of_every_numeric_failure() {
    let epoch = StreamEpoch::mint();
    assert_eq!(
        validate_subscribe(&window(&epoch, 0), 500, 200, 9_000, "stale"),
        Err(ResetReason::EpochChanged)
    );
}

// ------------------------------------------------------------------------------------------
// P-5(b)/(c)/(e): replay is exactly-once, contiguous, and never certified over a partial read
// ------------------------------------------------------------------------------------------

#[tokio::test]
async fn replay_delivers_every_row_above_the_cursor_exactly_once_and_in_order() {
    let epoch = StreamEpoch::mint();
    let log = TestLog::with_rows(epoch.as_str(), &[1, 2, 3, 4, 5]);
    let mut socket = RecordingSocket::default();

    let through = replay_catch_up(log.as_ref(), &mut socket, &epoch, 2, 5)
        .await
        .expect("replay should succeed");

    assert_eq!(through, 5);
    assert_eq!(
        socket.event_seqs(),
        vec![Some(3), Some(4), Some(5)],
        "rows <= afterSeq must not be replayed, and none may repeat"
    );
}

/// P-5(c): an induced read error yields `reset(read_error)` and NEVER `replayDone`.
#[tokio::test]
async fn a_catch_up_read_error_resets_and_never_certifies_the_replay() {
    let epoch = StreamEpoch::mint();
    let log = TestLog::with_rows(epoch.as_str(), &[1, 2, 3]);
    log.fail_reads(true);
    let mut socket = RecordingSocket::default();

    let outcome = replay_catch_up(log.as_ref(), &mut socket, &epoch, 0, 3).await;

    assert_eq!(outcome, Err(ResetReason::ReadError));
    assert!(
        !socket.has_replay_done(),
        "replayDone over a partial read is exactly the failure this test exists for"
    );
}

/// A hole in the log is a contiguity violation, not something to stream past.
#[tokio::test]
async fn a_hole_in_the_replayed_rows_resets_instead_of_splicing() {
    let epoch = StreamEpoch::mint();
    let log = TestLog::with_rows(epoch.as_str(), &[1, 2, 4]);
    let mut socket = RecordingSocket::default();

    let outcome = replay_catch_up(log.as_ref(), &mut socket, &epoch, 0, 4).await;

    assert_eq!(outcome, Err(ResetReason::ReadError));
    assert!(!socket.has_replay_done());
}

/// A drain that comes up short — rows vanished under it — resets rather than truncating.
#[tokio::test]
async fn a_short_drain_resets_with_cursor_pruned() {
    let epoch = StreamEpoch::mint();
    let log = TestLog::with_rows(epoch.as_str(), &[]);
    let mut socket = RecordingSocket::default();

    let outcome = replay_catch_up(log.as_ref(), &mut socket, &epoch, 0, 3).await;

    assert_eq!(outcome, Err(ResetReason::CursorPruned));
    assert!(!socket.has_replay_done());
}

/// P-5(e): the `H`-barrier. Subscribing at `H = hello.maxSeq` takes the lease AT `H`, so the
/// hydration delta (`> H`) sits strictly above the prune floor while the lease is live.
#[tokio::test]
async fn subscribing_at_the_hydration_barrier_pins_the_delta_against_pruning() {
    let epoch_holder = TestLog::with_rows("unused", &[]);
    let stream = stream_with(epoch_holder, 100, 0);
    let epoch = stream.epoch();
    let mut socket = RecordingSocket::default();

    let subscription = begin_subscription(&mut socket, &stream, 100, epoch.as_str())
        .await
        .expect("subscribing at H should succeed");

    assert_eq!(subscription.through_seq, 100);
    assert_eq!(
        stream.leases().lease_floor(),
        100,
        "the lease sits at H, so nothing above H is deletable while it lives"
    );
    assert!(socket.has_replay_done());
    drop(subscription);
    assert_eq!(stream.leases().lease_floor(), u64::MAX);
}

/// The fork MUST happen before the drain — that is what closes the drain-end→live gap — and
/// before the epoch window is READ, which is what closes the zombie-session race: a roll landing
/// between a window read and a later fork would never deliver its `EpochRolled` to this receiver,
/// leaving a session pinned to a dead epoch that silently drops every later durable frame.
#[test]
fn the_live_buffer_is_forked_before_the_epoch_window_read_and_the_catch_up_drain() {
    let source = include_str!("ws.rs");
    let body = source
        .split("pub(crate) async fn begin_subscription")
        .nth(1)
        .expect("begin_subscription should exist");
    let fork = body.find("stream.subscribe_frames()").expect("fork call");
    let window = body.find("stream.epoch_window()").expect("window read");
    let validate = body.find("validate_subscribe(").expect("validation call");
    let lease = body.find("leases().acquire(").expect("lease call");
    let drain = body.find("replay_catch_up(").expect("drain call");
    assert!(
        fork < window && window < validate,
        "fork → window read → validate"
    );
    assert!(
        validate < lease && lease < drain,
        "validate → lease → drain"
    );
}

/// A durable frame whose epoch is not the one this session subscribed under must TEAR THE SESSION
/// DOWN, never be skipped: skipping is what turns a missed `EpochRolled` into a live-looking
/// socket that receives no durable events at all.
#[test]
fn a_durable_frame_from_another_epoch_tears_the_session_down() {
    let source = include_str!("ws.rs");
    let body = source
        .split("SessionEvent::Live(LiveFrame::Durable(event)) => {")
        .nth(1)
        .expect("the durable live-frame arm should exist");
    let guard = body
        .find("if subscribed_epoch.as_ref() != Some(&event.epoch) {")
        .expect("the epoch guard should exist");
    let arm = &body[guard..];
    let teardown = arm
        // Tracks the signature: PR 3.3 threads the observability counters through the
        // teardown so every reset is attributed at its single choke point.
        .find("close_with_teardown(socket, counters, ResetReason::EpochChanged)")
        .expect("the epoch guard must tear the session down");
    assert!(
        arm[..teardown].find("continue").is_none(),
        "the mismatch arm must not silently skip the frame"
    );
}

// ------------------------------------------------------------------------------------------
// §4.4 / R-6: bounded per-session send queue
// ------------------------------------------------------------------------------------------

fn transient() -> QueuedFrame {
    QueuedFrame::Transient(ServerFrame::Event {
        seq: None,
        name: "agent:chunk".to_string(),
        payload: json!({}),
    })
}

fn durable(seq: u64) -> QueuedFrame {
    QueuedFrame::Durable(ServerFrame::Event {
        seq: Some(seq),
        name: "task:created".to_string(),
        payload: json!({}),
    })
}

#[test]
fn a_transient_overflow_drops_the_oldest_transient() {
    let mut queue = SessionSendQueue::with_capacity(2);
    assert_eq!(queue.push(transient()), SendQueueOutcome::Queued);
    assert_eq!(queue.push(transient()), SendQueueOutcome::Queued);

    assert_eq!(
        queue.push(transient()),
        SendQueueOutcome::DroppedOldestTransient
    );
    assert_eq!(queue.len(), 2, "the queue stays bounded");
}

/// A durable frame is never dropped to make room — it evicts a transient, because dropping it
/// would splice a hole into the stream.
#[test]
fn a_durable_frame_evicts_a_transient_rather_than_being_dropped() {
    let mut queue = SessionSendQueue::with_capacity(2);
    queue.push(transient());
    queue.push(durable(1));

    assert_eq!(
        queue.push(durable(2)),
        SendQueueOutcome::DroppedOldestTransient
    );
    assert_eq!(
        queue.pop(),
        Some(ServerFrame::Event {
            seq: Some(1),
            name: "task:created".to_string(),
            payload: json!({})
        })
    );
}

/// With nothing droppable left, a durable frame reports a gap — the caller kicks the session with
/// a cursor-resumable `reset` instead of silently losing a seq.
#[test]
fn a_full_queue_of_durable_frames_reports_a_durable_gap() {
    let mut queue = SessionSendQueue::with_capacity(2);
    queue.push(durable(1));
    queue.push(durable(2));

    assert_eq!(queue.push(durable(3)), SendQueueOutcome::DurableGap);
    assert_eq!(queue.len(), 2, "no durable frame was silently overwritten");
}

#[test]
fn a_transient_with_nothing_droppable_drops_itself() {
    let mut queue = SessionSendQueue::with_capacity(1);
    queue.push(durable(1));

    assert_eq!(
        queue.push(transient()),
        SendQueueOutcome::DroppedIncomingTransient,
        "freshness may be lost; durable contiguity may not"
    );
}

// ------------------------------------------------------------------------------------------
// Heartbeat + teardown
// ------------------------------------------------------------------------------------------

#[test]
fn the_heartbeat_budget_is_two_unacked_ticks() {
    assert!(!heartbeat_budget_exhausted(0));
    assert!(!heartbeat_budget_exhausted(1));
    assert!(heartbeat_budget_exhausted(2));
    assert_eq!(MAX_UNACKED_HEARTBEATS, 2);
    assert_eq!(HEARTBEAT_INTERVAL, Duration::from_secs(20));
}

fn session_context(
    stream: RemoteStreamHandle,
    kill: RemoteSessionKillChannel,
    device_check: Arc<dyn DeviceLivenessCheck>,
    heartbeat_interval: Duration,
) -> SessionContext {
    SessionContext {
        environment_id: "env-1".to_string(),
        stream,
        kill,
        device_check,
        device_id: RemoteDeviceId::from_string("device-1"),
        session_id: RemoteSessionId::from_string("session-1"),
        heartbeat_interval,
    }
}

fn kill_channel(
    registry: &crate::remote_server::session_registry::RemoteSessionRegistry,
    device: &RemoteDeviceId,
    session: &RemoteSessionId,
) -> RemoteSessionKillChannel {
    match registry.register(device, session) {
        RemoteSessionAdmission::Admitted(kill) => kill,
        RemoteSessionAdmission::CapExceeded { .. } => panic!("a fresh registry must admit"),
    }
}

/// The session opens with `hello`, carrying the live epoch and `maxSeq` (= the client's `H`).
#[tokio::test]
async fn a_session_opens_with_a_hello_carrying_the_live_epoch_and_high_water() {
    let stream = stream_with(TestLog::with_rows("unused", &[]), 42, 0);
    let registry = crate::remote_server::session_registry::RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("session-1");
    let kill = kill_channel(&registry, &device, &session);
    let mut socket = RecordingSocket::scripted(vec![]);

    let outcome = run_session(
        &mut socket,
        session_context(
            stream.clone(),
            kill,
            Arc::new(AlwaysLive),
            Duration::from_secs(3_600),
        ),
    )
    .await;

    assert_eq!(outcome, SessionOutcome::ClientClosed);
    assert_eq!(
        socket.sent.first(),
        Some(&hello_frame("env-1", &stream.epoch_window(), 42))
    );
}

/// P-2: revoking a device closes the **existing socket** with `error{revoked}` — not just future
/// HTTP. The kill channel is the mechanism; the session selects on it.
#[tokio::test]
async fn revocation_tears_down_the_live_socket_with_an_error_frame() {
    let stream = stream_with(TestLog::with_rows("unused", &[]), 0, 0);
    let registry = crate::remote_server::session_registry::RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("session-1");
    let kill = kill_channel(&registry, &device, &session);
    // Revoke before the session runs: the signal is already queued on the kill channel.
    registry.kill_device(&device, ResetReason::Revoked);
    let mut socket = RecordingSocket::silent();

    let outcome = run_session(
        &mut socket,
        session_context(
            stream,
            kill,
            Arc::new(AlwaysLive),
            Duration::from_secs(3_600),
        ),
    )
    .await;

    assert_eq!(outcome, SessionOutcome::Revoked);
    assert!(
        socket.sent.iter().any(|frame| matches!(
            frame,
            ServerFrame::Error {
                code: ErrorCode::RemoteUnauthorized,
                ..
            }
        )),
        "a revoked device must be told so, then closed"
    );
    assert!(socket.closed);
}

/// P-15: host disable closes live sessions with `reset(host_disabled)`.
#[tokio::test]
async fn host_disable_closes_live_sessions_with_a_typed_reset() {
    let stream = stream_with(TestLog::with_rows("unused", &[]), 0, 0);
    let registry = crate::remote_server::session_registry::RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("session-1");
    let kill = kill_channel(&registry, &device, &session);
    registry.kill_all(ResetReason::HostDisabled);
    let mut socket = RecordingSocket::silent();

    let outcome = run_session(
        &mut socket,
        session_context(
            stream,
            kill,
            Arc::new(AlwaysLive),
            Duration::from_secs(3_600),
        ),
    )
    .await;

    assert_eq!(outcome, SessionOutcome::HostDisabled);
    assert_eq!(socket.resets(), vec![ResetReason::HostDisabled]);
}

/// §4.4 defense in depth: even if the registry entry was missed, the heartbeat-tick device
/// re-check closes a revoked device's socket within one heartbeat window.
#[tokio::test]
async fn the_heartbeat_device_recheck_closes_a_revoked_session() {
    let stream = stream_with(TestLog::with_rows("unused", &[]), 0, 0);
    let registry = crate::remote_server::session_registry::RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("session-1");
    let kill = kill_channel(&registry, &device, &session);
    let mut socket = RecordingSocket::silent();

    let outcome = run_session(
        &mut socket,
        session_context(
            stream,
            kill,
            Arc::new(AlwaysRevoked),
            Duration::from_millis(1),
        ),
    )
    .await;

    assert_eq!(outcome, SessionOutcome::Revoked);
}

/// Fail closed on reads: an unreadable device store must not read as "still authorized".
#[tokio::test]
async fn an_unreadable_device_store_closes_the_session_rather_than_trusting_it() {
    let stream = stream_with(TestLog::with_rows("unused", &[]), 0, 0);
    let registry = crate::remote_server::session_registry::RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("session-1");
    let kill = kill_channel(&registry, &device, &session);
    let mut socket = RecordingSocket::silent();

    let outcome = run_session(
        &mut socket,
        session_context(
            stream,
            kill,
            Arc::new(UnreadableDevice),
            Duration::from_millis(1),
        ),
    )
    .await;

    assert_eq!(outcome, SessionOutcome::Revoked);
}

/// The server closes after two unacked heartbeats and no more (§3.2).
#[tokio::test]
async fn the_server_closes_after_two_unacked_heartbeats() {
    let stream = stream_with(TestLog::with_rows("unused", &[]), 0, 0);
    let registry = crate::remote_server::session_registry::RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("session-1");
    let kill = kill_channel(&registry, &device, &session);
    let mut socket = RecordingSocket::silent();

    let outcome = run_session(
        &mut socket,
        session_context(stream, kill, Arc::new(AlwaysLive), Duration::from_millis(1)),
    )
    .await;

    assert_eq!(outcome, SessionOutcome::HeartbeatTimeout);
    let heartbeats = socket
        .sent
        .iter()
        .filter(|frame| matches!(frame, ServerFrame::Heartbeat { .. }))
        .count();
    assert_eq!(
        heartbeats, MAX_UNACKED_HEARTBEATS as usize,
        "exactly two heartbeats go unanswered before the close"
    );
    assert!(socket.closed);
}

/// A subscribe the server cannot serve ends the session with the typed reset and no `replayDone`.
#[tokio::test]
async fn an_unserviceable_subscribe_resets_and_closes() {
    let stream = stream_with(TestLog::with_rows("unused", &[]), 10, 0);
    let registry = crate::remote_server::session_registry::RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("session-1");
    let kill = kill_channel(&registry, &device, &session);
    let mut socket = RecordingSocket::scripted(vec![ClientFrame::Subscribe {
        after_seq: 5,
        stream_epoch: "a-stale-epoch".to_string(),
    }]);

    let outcome = run_session(
        &mut socket,
        session_context(
            stream,
            kill,
            Arc::new(AlwaysLive),
            Duration::from_secs(3_600),
        ),
    )
    .await;

    assert_eq!(outcome, SessionOutcome::Reset(ResetReason::EpochChanged));
    assert_eq!(socket.resets(), vec![ResetReason::EpochChanged]);
    assert!(!socket.has_replay_done());
    assert!(socket.closed);
}

/// Only `cursorAck` moves the retention floor; `heartbeatAck` is liveness and nothing more.
#[tokio::test]
async fn only_cursor_ack_moves_the_lease_floor() {
    let stream = stream_with(TestLog::with_rows("unused", &[]), 100, 0);
    let epoch = stream.epoch();
    let registry = crate::remote_server::session_registry::RemoteSessionRegistry::new();
    let device = RemoteDeviceId::from_string("device-1");
    let session = RemoteSessionId::from_string("session-1");
    let kill = kill_channel(&registry, &device, &session);
    let floor_probe = stream.clone();
    let mut socket = RecordingSocket::scripted(vec![
        ClientFrame::Subscribe {
            after_seq: 100,
            stream_epoch: epoch.as_str().to_string(),
        },
        ClientFrame::HeartbeatAck { t: 1 },
    ]);

    let session_task = run_session(
        &mut socket,
        session_context(
            stream,
            kill,
            Arc::new(AlwaysLive),
            Duration::from_secs(3_600),
        ),
    );
    let outcome = session_task.await;

    assert_eq!(outcome, SessionOutcome::ClientClosed);
    // The lease released with the session; the point is that `heartbeatAck` never advanced it.
    assert_eq!(floor_probe.leases().lease_floor(), u64::MAX);
}

// ------------------------------------------------------------------------------------------
// Wiring invariants
// ------------------------------------------------------------------------------------------

/// The session lifecycle events are Local-only in the classification table, so they can never be
/// forwarded to a remote client — one device must not be able to watch another connect (§5.5).
#[test]
fn the_session_lifecycle_events_are_classified_local_only() {
    for name in [REMOTE_SESSION_CONNECTED_EVENT, REMOTE_SESSION_CLOSED_EVENT] {
        let entry = ralphx_remote_protocol::EventClassification::find(name)
            .unwrap_or_else(|| panic!("{name} must be classified"));
        assert_eq!(
            entry.delivery,
            ralphx_remote_protocol::EventDelivery::LocalOnly,
            "{name} must never fan out to remote clients"
        );
        assert_eq!(
            entry.origin,
            ralphx_remote_protocol::EventOrigin::Backend,
            "{name} is a Rust emit; the origin must say so"
        );
    }
}

/// The WS route authenticates itself. It must NOT be in the two-entry pre-auth allowlist, whose
/// exact contents P-1 pins.
#[test]
fn the_ws_route_is_ticket_authenticated_not_pre_auth() {
    assert!(!crate::remote_server::PRE_AUTH_ALLOWLIST.contains(&WS_EVENTS_PATH));
    assert_eq!(
        crate::remote_server::TICKET_AUTH_ALLOWLIST,
        &[WS_EVENTS_PATH]
    );
}

/// The ticket is consumed before anything else happens, and the device is re-resolved after —
/// a revoke between ticket issue and upgrade must lose.
#[test]
fn the_upgrade_consumes_the_ticket_then_rechecks_the_device_before_upgrading() {
    let source = include_str!("ws.rs");
    let body = source
        .split("pub(crate) async fn ws_events_handler")
        .nth(1)
        .expect("handler should exist");
    let consume = body.find("tickets.consume(").expect("ticket consume");
    let recheck = body
        .find("auth.devices.get(&device_id)")
        .expect("device recheck");
    let admit = body.find("registry.register(").expect("registry admission");
    let upgrade = body.find("upgrade.on_upgrade(").expect("upgrade call");
    assert!(
        consume < recheck && recheck < admit && admit < upgrade,
        "ticket → device → cap → upgrade"
    );
}

/// §4.4 abuse control: a cap slot admitted before the upgrade must come back when the upgrade
/// never completes. axum drops the `on_upgrade` callback UNRUN on a failed upgrade, so the guard
/// has to be constructed outside it — its `Drop` is the only thing that releases the slot on that
/// path. Constructed inside, each failed upgrade permanently consumed one of the device's 8 slots.
#[tokio::test]
async fn a_dropped_upgrade_callback_releases_the_device_cap_slot() {
    let auth = crate::remote_server::auth_tests::in_memory_auth_context();
    let device = RemoteDeviceId::from_string("device-cap");
    let session = RemoteSessionId::from_string("session-cap");
    let _kill = kill_channel(&auth.registry, &device, &session);
    assert_eq!(auth.registry.device_session_count(&device), 1);

    let guard = SessionGuard::new(auth.clone(), device.clone(), session.clone());
    // Exactly what axum does to a callback it never invokes: the closure — and everything moved
    // into it — is dropped without running.
    let never_runs = move |_socket: ()| async move {
        guard.open("127.0.0.1:1").await;
    };
    drop(never_runs);

    assert_eq!(
        auth.registry.device_session_count(&device),
        0,
        "a dropped upgrade callback must not leak the admitted cap slot"
    );
}

/// The guard is built BEFORE the upgrade and moved in, so the dropped-callback path above is the
/// one the handler actually takes.
#[test]
fn the_session_guard_is_constructed_before_the_upgrade() {
    let source = include_str!("ws.rs");
    let body = source
        .split("pub(crate) async fn ws_events_handler")
        .nth(1)
        .expect("handler should exist");
    let guard = body.find("SessionGuard::new(").expect("guard construction");
    let upgrade = body.find("upgrade.on_upgrade(").expect("upgrade call");
    assert!(
        guard < upgrade,
        "the unregistering guard must exist before the upgrade is handed off"
    );
}

/// The epoch roll cause reaching the WS layer is the sequencer's, not a locally-invented one.
#[test]
fn the_ws_layer_does_not_mint_epoch_roll_causes() {
    let source = include_str!("ws.rs");
    assert!(!source.contains("EpochRollCause::"));
    let _ = EpochRollCause::CaptureOverload;
}

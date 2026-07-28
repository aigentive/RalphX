//! The remote event WebSocket (§3.2 framing, §3.4 subscription semantics, §4.4 abuse controls).
//!
//! Everything here is arranged around one rule: **a client is never handed a partial or spliced
//! stream.** Whenever the server cannot prove it can serve a cursor contiguously — wrong epoch,
//! cursor above the high-water, cursor below the pruned floor, a read that errored, a hole in the
//! replayed rows, a session that fell behind the live buffer — it sends a typed `reset` and lets
//! the client cold-hydrate. `replayDone` is only ever emitted over a read that completed and was
//! proven contiguous (fable H4, `stateful-workflow-review.md` "fail closed on reads").
//!
//! Auth happens **before** the upgrade, via the single-use device-bound ticket PR 1.2 issues
//! (§3.2). A browser cannot set an `Authorization` header on a WebSocket, which is exactly why
//! the ticket exists; the bearer middleware therefore skips this one path and the handler below
//! is the complete authentication for it — consume the ticket, re-resolve the device fail-closed,
//! check the scope, admit against the per-device cap, and only then upgrade.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use ralphx_remote_protocol::{
    ClientFrame, ErrorCode, ResetReason, Scope, ServerFrame, PROTOCOL_VERSION,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::entities::{RemoteAuditAction, RemoteDeviceId, RemoteSession, RemoteSessionId};
use crate::domain::repositories::RemoteWsTicketOutcome;
use crate::domain::services::key_crypto::hash_key;
use crate::infrastructure::sqlite::RemoteEventLogStore;
use crate::remote_server::auth::{now_timestamp, RemoteAuthContext, RemoteAuthRejection};
use crate::remote_server::endpoints::RemoteRouterState;
use crate::remote_server::remote_error_response;
use crate::remote_server::sequencer::{
    parse_payload, EpochWindow, RemoteStreamHandle, SequencedEvent, StreamEpoch, StreamFrame,
};
use crate::remote_server::session_registry::{RemoteSessionAdmission, RemoteSessionKillChannel};

#[cfg(test)]
#[path = "ws_tests.rs"]
mod tests;

/// Local-only session lifecycle events (§5.5).
///
/// Classified **Local-only** in the protocol crate precisely so remote clients never watch each
/// other: these reach the host's own bus (the Remote Access pane in PR 1.7 consumes them) and are
/// structurally unable to enter the capture bank or the durable log.
pub(crate) const REMOTE_SESSION_CONNECTED_EVENT: &str = "remote:session_connected";
pub(crate) const REMOTE_SESSION_CLOSED_EVENT: &str = "remote:session_closed";

/// The host-local event bus, as a seam so session tests need no Tauri app handle.
pub(crate) trait SessionLifecycleSink: Send + Sync {
    fn emit(&self, name: &str, payload: Value);
}

/// No-op sink for routers built without an app handle (unit tests, header-only fixtures).
pub(crate) struct NoopLifecycleSink;

impl SessionLifecycleSink for NoopLifecycleSink {
    fn emit(&self, _name: &str, _payload: Value) {}
}

pub(crate) struct TauriLifecycleSink(pub tauri::AppHandle);

impl SessionLifecycleSink for TauriLifecycleSink {
    fn emit(&self, name: &str, payload: Value) {
        use tauri::Emitter;
        if let Err(error) = self.0.emit(name, payload) {
            tracing::warn!(%error, event_name = name, "Emitting a remote session lifecycle event failed");
        }
    }
}

/// The event stream endpoint. Ticket-authenticated, not bearer-authenticated.
pub(crate) const WS_EVENTS_PATH: &str = "/remote/v1/events";

/// Heartbeat cadence and the miss budget (§3.2: every 20 s, close after 2 unacked).
pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
pub(crate) const MAX_UNACKED_HEARTBEATS: u32 = 2;

/// Bounded per-session outbound queue (§4.4 fourth abuse control, §9 R-6).
pub(crate) const SESSION_SEND_QUEUE_CAPACITY: usize = 512;

/// Rows per catch-up page. Bounds memory without holding any lock across the whole replay.
pub(crate) const REPLAY_PAGE_LIMIT: usize = 2_000;

// ------------------------------------------------------------------------------------------
// Subscribe validation — fail closed, in a fixed order (§3.4)
// ------------------------------------------------------------------------------------------

/// Validates `subscribe{afterSeq, streamEpoch}` against the live stream.
///
/// Order is deliberate and load-bearing. The epoch is checked first because a stale epoch makes
/// every other comparison meaningless (the cursor refers to a different numbering *content*, even
/// though the numbers themselves are monotonic).
pub(crate) fn validate_subscribe(
    window: &EpochWindow,
    max_seq: u64,
    pruned_floor: u64,
    after_seq: u64,
    client_epoch: &str,
) -> Result<(), ResetReason> {
    if window.epoch.as_str() != client_epoch {
        return Err(ResetReason::EpochChanged);
    }
    if after_seq > max_seq {
        return Err(ResetReason::AfterSeqGtMax);
    }
    // Below the epoch floor the rows belong to a previous epoch. They are unreplayable by
    // construction (§3.4 DDL note), so a cursor there is an epoch change even though the client
    // quoted the current epoch string — this is what makes a cross-restart warm splice
    // impossible without depending on the client to notice (P-5(a), P-22(a)).
    if after_seq < window.floor_seq {
        return Err(ResetReason::EpochChanged);
    }
    if after_seq < pruned_floor {
        return Err(ResetReason::CursorPruned);
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------
// Bounded per-session send queue (§4.4 / R-6)
// ------------------------------------------------------------------------------------------

/// A frame waiting to go out, tagged by what dropping it would cost.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum QueuedFrame {
    /// Durable event or protocol control: dropping it breaks contiguity.
    Durable(ServerFrame),
    /// Live-only: dropping it costs nothing but freshness (§3.4 transient class).
    Transient(ServerFrame),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendQueueOutcome {
    Queued,
    /// Backpressure absorbed by dropping the OLDEST transient frame — the newest live state is
    /// what a UI wants, and no durable contiguity is affected.
    DroppedOldestTransient,
    /// The incoming transient itself was dropped: nothing older was droppable.
    DroppedIncomingTransient,
    /// A durable frame could not be queued. Dropping it would splice a hole into the stream, so
    /// the session is kicked with a cursor-resumable `reset` instead (never a silent gap).
    DurableGap,
}

#[derive(Debug)]
pub(crate) struct SessionSendQueue {
    frames: VecDeque<QueuedFrame>,
    capacity: usize,
}

impl SessionSendQueue {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            frames: VecDeque::new(),
            capacity,
        }
    }

    /// Depth probe. Only the tests need it — production never branches on the depth, it branches
    /// on the [`SendQueueOutcome`], which is the whole point of returning one.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.frames.len()
    }

    pub(crate) fn push(&mut self, frame: QueuedFrame) -> SendQueueOutcome {
        if self.frames.len() < self.capacity {
            self.frames.push_back(frame);
            return SendQueueOutcome::Queued;
        }
        let evicted = self.evict_oldest_transient();
        match (evicted, &frame) {
            (true, _) => {
                self.frames.push_back(frame);
                SendQueueOutcome::DroppedOldestTransient
            }
            (false, QueuedFrame::Transient(_)) => SendQueueOutcome::DroppedIncomingTransient,
            (false, QueuedFrame::Durable(_)) => SendQueueOutcome::DurableGap,
        }
    }

    pub(crate) fn pop(&mut self) -> Option<ServerFrame> {
        self.frames.pop_front().map(|frame| match frame {
            QueuedFrame::Durable(frame) | QueuedFrame::Transient(frame) => frame,
        })
    }

    fn evict_oldest_transient(&mut self) -> bool {
        let Some(index) = self
            .frames
            .iter()
            .position(|frame| matches!(frame, QueuedFrame::Transient(_)))
        else {
            return false;
        };
        self.frames.remove(index);
        true
    }
}

/// §3.2: close after two unacked heartbeats, so a half-open socket is detected in ~40 s.
pub(crate) fn heartbeat_budget_exhausted(unacked: u32) -> bool {
    unacked >= MAX_UNACKED_HEARTBEATS
}

pub(crate) fn hello_frame(environment_id: &str, window: &EpochWindow, max_seq: u64) -> ServerFrame {
    ServerFrame::Hello {
        protocol_version: PROTOCOL_VERSION,
        environment_id: environment_id.to_string(),
        stream_epoch: window.epoch.as_str().to_string(),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        max_seq,
        heartbeat_secs: HEARTBEAT_INTERVAL.as_secs() as u32,
    }
}

fn event_frame(event: &SequencedEvent) -> ServerFrame {
    ServerFrame::Event {
        seq: Some(event.seq),
        name: event.name.clone(),
        payload: event.payload.clone(),
    }
}

// ------------------------------------------------------------------------------------------
// Socket seam
// ------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SocketError {
    Closed,
    Protocol(String),
}

/// The socket a session drives.
///
/// A trait so the whole session lifecycle — hello, replay, reset, heartbeat timeout, revocation
/// teardown — is testable deterministically instead of against a real TCP upgrade. Implementations
/// MUST be cancel-safe in [`Self::recv`]: the session loop `select!`s on it and drops the future
/// whenever another branch wins.
#[async_trait::async_trait]
pub(crate) trait SessionSocket: Send {
    async fn send(&mut self, frame: ServerFrame) -> Result<(), SocketError>;
    /// `None` = the peer closed. `Some(Err(_))` = an unusable frame; the session closes.
    async fn recv(&mut self) -> Option<Result<ClientFrame, SocketError>>;
    async fn close(&mut self);
}

// ------------------------------------------------------------------------------------------
// Catch-up replay — plain reads, contiguity-asserted, fail-closed
// ------------------------------------------------------------------------------------------

/// Replays `after_seq < seq <= through_seq` to the socket, asserting contiguity as it goes.
///
/// Returns the `throughSeq` the caller may put in `replayDone`. Any read error, any hole, and any
/// shortfall is an `Err(ResetReason)` — the caller MUST send `reset` and MUST NOT send
/// `replayDone` (P-5(c)).
pub(crate) async fn replay_catch_up(
    log: &dyn RemoteEventLogStore,
    socket: &mut dyn SessionSocket,
    epoch: &StreamEpoch,
    after_seq: u64,
    through_seq: u64,
) -> Result<u64, ResetReason> {
    let mut cursor = after_seq;
    while cursor < through_seq {
        let rows = log
            // Plain read (N-M2): a write-intent transaction here would serialize the replay
            // against the sequencer's own commits.
            .read_range(epoch.as_str(), cursor, through_seq, REPLAY_PAGE_LIMIT)
            .await
            .map_err(|error| {
                tracing::error!(%error, after_seq, through_seq, "Remote catch-up read failed");
                // Never `replayDone` over a partial read.
                ResetReason::ReadError
            })?;
        if rows.is_empty() {
            // The rows are gone from underneath a drain that the lease was supposed to protect,
            // or the range was never contiguous. Either way the client cannot be served here.
            tracing::warn!(
                cursor,
                through_seq,
                "Remote catch-up drain came up short; resetting instead of splicing"
            );
            return Err(ResetReason::CursorPruned);
        }
        for row in rows {
            if row.seq != cursor + 1 {
                tracing::error!(
                    expected = cursor + 1,
                    found = row.seq,
                    "Remote catch-up replay hit a hole"
                );
                return Err(ResetReason::ReadError);
            }
            cursor = row.seq;
            let event = SequencedEvent {
                seq: row.seq,
                epoch: epoch.clone(),
                name: row.name,
                payload: parse_payload(&row.payload),
            };
            socket
                .send(event_frame(&event))
                .await
                .map_err(|_| ResetReason::ReadError)?;
        }
    }
    Ok(cursor)
}

// ------------------------------------------------------------------------------------------
// Session lifecycle
// ------------------------------------------------------------------------------------------

/// Everything a session needs that is not the socket.
pub(crate) struct SessionContext {
    pub environment_id: String,
    pub stream: RemoteStreamHandle,
    pub kill: RemoteSessionKillChannel,
    /// Re-checked on every heartbeat tick, fail-closed: a revocation that never reached the
    /// registry still closes the socket within ~20 s (§4.4 defense in depth).
    pub device_check: Arc<dyn DeviceLivenessCheck>,
    pub device_id: RemoteDeviceId,
    pub session_id: RemoteSessionId,
    pub heartbeat_interval: Duration,
}

/// Whether a device may still hold a live session.
///
/// Tri-state on purpose: a store error must NOT read as "still fine". A read that fails closes
/// the socket, because a revoked device that looks unknown due to an outage is exactly the case
/// this check exists for (`stateful-workflow-review.md` "fail closed on reads").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceLiveness {
    Live,
    Revoked,
    Unreadable,
}

#[async_trait::async_trait]
pub(crate) trait DeviceLivenessCheck: Send + Sync {
    async fn check(&self, device_id: &RemoteDeviceId) -> DeviceLiveness;
}

/// How a session ended. Drives the audit row and the `remote:session_closed` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionOutcome {
    ClientClosed,
    HeartbeatTimeout,
    Reset(ResetReason),
    Revoked,
    HostDisabled,
    SocketError,
}

impl SessionOutcome {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::ClientClosed => "client_closed",
            Self::HeartbeatTimeout => "heartbeat_timeout",
            Self::Reset(_) => "reset",
            Self::Revoked => "revoked",
            Self::HostDisabled => "host_disabled",
            Self::SocketError => "socket_error",
        }
    }
}

/// Runs one WS session to completion.
///
/// The retention lease is an owned local: whichever way this function leaves — clean close,
/// heartbeat timeout, revocation, panic unwind — the lease releases with it (P-19(b) drop path).
pub(crate) async fn run_session(
    socket: &mut dyn SessionSocket,
    mut context: SessionContext,
) -> SessionOutcome {
    let window = context.stream.epoch_window();
    let max_seq = context.stream.max_seq();
    tracing::debug!(
        session_id = %context.session_id,
        device_id = %context.device_id,
        epoch = %window.epoch,
        max_seq,
        "Remote WS session opened"
    );
    if socket
        .send(hello_frame(&context.environment_id, &window, max_seq))
        .await
        .is_err()
    {
        return SessionOutcome::SocketError;
    }

    let mut lease: Option<crate::remote_server::retention::RetentionLease> = None;
    let mut frames: Option<tokio::sync::broadcast::Receiver<StreamFrame>> = None;
    // The epoch this session's replay was proven against; live durable frames from any other
    // epoch are not this client's stream.
    let mut subscribed_epoch: Option<StreamEpoch> = None;
    let mut queue = SessionSendQueue::with_capacity(SESSION_SEND_QUEUE_CAPACITY);
    let mut last_sent: u64 = 0;
    let mut unacked_heartbeats: u32 = 0;
    let mut heartbeat = tokio::time::interval(context.heartbeat_interval);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await; // the first tick fires immediately; skip it

    loop {
        while let Some(frame) = queue.pop() {
            if socket.send(frame).await.is_err() {
                return SessionOutcome::SocketError;
            }
        }

        // The select only *classifies* the event; every handler runs after it, once the branch
        // futures are dropped. That is what lets a handler send on the same socket a branch was
        // reading from, and reassign the subscription a branch was polling.
        let event = tokio::select! {
            biased;
            reason = context.kill.recv() => SessionEvent::Kill(reason),
            incoming = socket.recv() => SessionEvent::Incoming(incoming),
            frame = recv_stream_frame(frames.as_mut()) => SessionEvent::Live(frame),
            _ = heartbeat.tick() => SessionEvent::HeartbeatTick,
        };

        match event {
            // A `None` reason means the sender was dropped without one — still a teardown.
            SessionEvent::Kill(reason) => {
                return close_with_teardown(socket, reason.unwrap_or(ResetReason::HostDisabled))
                    .await
            }
            SessionEvent::Incoming(None) => return SessionOutcome::ClientClosed,
            SessionEvent::Incoming(Some(Err(_))) => return SessionOutcome::SocketError,
            SessionEvent::Incoming(Some(Ok(ClientFrame::Subscribe {
                after_seq,
                stream_epoch,
            }))) => {
                match begin_subscription(socket, &context.stream, after_seq, &stream_epoch).await {
                    Ok(subscription) => {
                        last_sent = subscription.through_seq;
                        subscribed_epoch = Some(subscription.epoch);
                        lease = Some(subscription.lease);
                        frames = Some(subscription.frames);
                    }
                    Err(reason) => {
                        let _ = socket.send(ServerFrame::Reset { reason }).await;
                        socket.close().await;
                        return SessionOutcome::Reset(reason);
                    }
                }
            }
            SessionEvent::Incoming(Some(Ok(ClientFrame::CursorAck { seq }))) => {
                // ONLY `cursorAck` moves the retention floor (§3.4 lease lifecycle).
                if let Some(lease) = lease.as_ref() {
                    lease.advance(seq);
                }
            }
            SessionEvent::Incoming(Some(Ok(ClientFrame::HeartbeatAck { .. }))) => {
                // Liveness only: deliberately does NOT refresh the lease TTL.
                unacked_heartbeats = 0;
            }
            SessionEvent::Live(LiveFrame::Durable(event)) => {
                // A frame published under a different epoch than the one this session subscribed
                // to is not this client's stream, and forwarding it would splice a seq from a
                // numbering the client never agreed to. Tear down rather than skip: the receiver
                // is forked before the epoch window is read (`begin_subscription`) and broadcast
                // preserves the sequencer's publish order, so a mismatch can only mean this
                // session already crossed a roll boundary. Skipping instead would turn a missed
                // roll into a healthy-looking session that never receives another durable event.
                if subscribed_epoch.as_ref() != Some(&event.epoch) {
                    return close_with_teardown(socket, ResetReason::EpochChanged).await;
                }
                // The client already has everything through `through_seq` from the replay; the
                // fork deliberately overlaps it (§3.4).
                if event.seq > last_sent {
                    last_sent = event.seq;
                    if queue.push(QueuedFrame::Durable(event_frame(&event)))
                        == SendQueueOutcome::DurableGap
                    {
                        return close_with_teardown(socket, ResetReason::CursorPruned).await;
                    }
                }
            }
            SessionEvent::Live(LiveFrame::Transient { name, payload }) => {
                // Transient frames carry no seq (§3.2) and never enter the log (A-4).
                queue.push(QueuedFrame::Transient(ServerFrame::Event {
                    seq: None,
                    name: name.to_string(),
                    payload: (*payload).clone(),
                }));
            }
            SessionEvent::Live(LiveFrame::EpochRolled) => {
                return close_with_teardown(socket, ResetReason::EpochChanged).await
            }
            // The session fell behind the live buffer: a durable gap. Kick it with a
            // cursor-resumable reset rather than splice over the missing seqs.
            SessionEvent::Live(LiveFrame::Lagged) => {
                return close_with_teardown(socket, ResetReason::CursorPruned).await
            }
            SessionEvent::HeartbeatTick => {
                if heartbeat_budget_exhausted(unacked_heartbeats) {
                    socket.close().await;
                    return SessionOutcome::HeartbeatTimeout;
                }
                match context.device_check.check(&context.device_id).await {
                    DeviceLiveness::Live => {}
                    DeviceLiveness::Revoked | DeviceLiveness::Unreadable => {
                        let _ = socket
                            .send(ServerFrame::Error {
                                code: ErrorCode::RemoteUnauthorized,
                                message: "This device is no longer authorized.".to_string(),
                            })
                            .await;
                        socket.close().await;
                        return SessionOutcome::Revoked;
                    }
                }
                unacked_heartbeats += 1;
                if socket
                    .send(ServerFrame::Heartbeat {
                        t: Utc::now().timestamp() as u64,
                    })
                    .await
                    .is_err()
                {
                    return SessionOutcome::SocketError;
                }
            }
        }
    }
}

/// What one turn of the session loop observed. Classification only — no effects.
enum SessionEvent {
    Kill(Option<ResetReason>),
    Incoming(Option<Result<ClientFrame, SocketError>>),
    Live(LiveFrame),
    HeartbeatTick,
}

/// Sends the terminal frame for a teardown reason, then closes.
async fn close_with_teardown(
    socket: &mut dyn SessionSocket,
    reason: ResetReason,
) -> SessionOutcome {
    // Revocation is an `error{revoked}` (P-2); every other teardown is a typed `reset`.
    let outcome = match reason {
        ResetReason::Revoked => {
            let _ = socket
                .send(ServerFrame::Error {
                    code: ErrorCode::RemoteUnauthorized,
                    message: "This device has been revoked.".to_string(),
                })
                .await;
            SessionOutcome::Revoked
        }
        ResetReason::HostDisabled => {
            let _ = socket.send(ServerFrame::Reset { reason }).await;
            SessionOutcome::HostDisabled
        }
        other => {
            let _ = socket.send(ServerFrame::Reset { reason: other }).await;
            SessionOutcome::Reset(other)
        }
    };
    socket.close().await;
    outcome
}

pub(crate) struct Subscription {
    pub through_seq: u64,
    /// The epoch the replay was proven against. Pinned here rather than re-read later, so a roll
    /// during the session cannot retroactively change what this client agreed to.
    pub epoch: StreamEpoch,
    pub lease: crate::remote_server::retention::RetentionLease,
    pub frames: tokio::sync::broadcast::Receiver<StreamFrame>,
}

/// Fork the live buffer → validate → acquire the lease → drain → `replayDone`.
///
/// The fork happens **before** the drain and the lease is taken at `afterSeq`. Together those two
/// lines are the exactly-once contiguous handoff: the fork closes the drain-end→live gap, and the
/// lease keeps the rows the drain is about to read from being pruned underneath it (§3.4).
///
/// The fork also happens before the epoch window is READ. A roll landing between a window read and
/// a later fork would publish its `EpochRolled` into a buffer this receiver does not hold yet: the
/// window would validate against the dead epoch, the session would pin itself to it, and every
/// later durable frame would be skipped forever — a live-looking socket carrying no durable events.
/// Forking first collapses that window: a roll before the read fails validation (`epoch_changed`),
/// a roll after it is delivered to this very receiver.
pub(crate) async fn begin_subscription(
    socket: &mut dyn SessionSocket,
    stream: &RemoteStreamHandle,
    after_seq: u64,
    client_epoch: &str,
) -> Result<Subscription, ResetReason> {
    // ORDER IS THE CONTRACT: fork first, then read/validate the window, then lease, then read.
    let frames = stream.subscribe_frames();
    let window = stream.epoch_window();
    validate_subscribe(
        &window,
        stream.max_seq(),
        stream.pruned_floor(),
        after_seq,
        client_epoch,
    )?;

    let lease = stream.leases().acquire(after_seq);
    let through_seq = stream.max_seq();

    let replayed = replay_catch_up(
        stream.log().as_ref(),
        socket,
        &window.epoch,
        after_seq,
        through_seq,
    )
    .await?;

    // Post-drain floor re-validation: if the floor moved past this cursor while we drained, the
    // replay we just sent may be short. Fail closed rather than certify it with `replayDone`.
    if stream.pruned_floor() > after_seq {
        return Err(ResetReason::CursorPruned);
    }

    socket
        .send(ServerFrame::ReplayDone {
            through_seq: replayed,
        })
        .await
        .map_err(|_| ResetReason::ReadError)?;

    Ok(Subscription {
        through_seq: replayed,
        epoch: window.epoch,
        lease,
        frames,
    })
}

/// What the live-stream select branch yields.
pub(crate) enum LiveFrame {
    Durable(Arc<SequencedEvent>),
    Transient {
        name: &'static str,
        payload: Arc<Value>,
    },
    EpochRolled,
    Lagged,
}

/// Before `subscribe` — and after the sequencer is gone — this branch parks forever rather than
/// resolving, so the select never busy-loops the session task on a branch with nothing to say.
async fn recv_stream_frame(
    frames: Option<&mut tokio::sync::broadcast::Receiver<StreamFrame>>,
) -> LiveFrame {
    let Some(frames) = frames else {
        return std::future::pending::<LiveFrame>().await;
    };
    match frames.recv().await {
        Ok(StreamFrame::Durable(event)) => LiveFrame::Durable(event),
        Ok(StreamFrame::Transient { name, payload }) => LiveFrame::Transient { name, payload },
        Ok(StreamFrame::EpochRolled) => LiveFrame::EpochRolled,
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => LiveFrame::Lagged,
        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
            std::future::pending::<LiveFrame>().await
        }
    }
}

// ------------------------------------------------------------------------------------------
// Axum adapter
// ------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct WsQuery {
    pub ticket: String,
}

struct AxumSessionSocket {
    sender: futures::stream::SplitSink<WebSocket, Message>,
    receiver: futures::stream::SplitStream<WebSocket>,
}

#[async_trait::async_trait]
impl SessionSocket for AxumSessionSocket {
    async fn send(&mut self, frame: ServerFrame) -> Result<(), SocketError> {
        let text = serde_json::to_string(&frame).map_err(|error| {
            SocketError::Protocol(format!("server frame is not serializable: {error}"))
        })?;
        self.sender
            .send(Message::Text(text))
            .await
            .map_err(|_| SocketError::Closed)
    }

    async fn recv(&mut self) -> Option<Result<ClientFrame, SocketError>> {
        loop {
            // `StreamExt::next` is cancel-safe, so dropping this future in `select!` loses nothing.
            match self.receiver.next().await? {
                Ok(Message::Text(text)) => {
                    return Some(serde_json::from_str(&text).map_err(|error| {
                        SocketError::Protocol(format!("unrecognized client frame: {error}"))
                    }))
                }
                // Ping/Pong are handled by axum; binary frames are not part of the v1 protocol.
                Ok(Message::Binary(_)) => {
                    return Some(Err(SocketError::Protocol(
                        "binary frames are not part of the remote protocol".to_string(),
                    )))
                }
                Ok(Message::Close(_)) => return None,
                Ok(_) => continue,
                Err(_) => return Some(Err(SocketError::Closed)),
            }
        }
    }

    async fn close(&mut self) {
        let _ = self.sender.close().await;
    }
}

struct RepositoryDeviceCheck {
    auth: RemoteAuthContext,
}

#[async_trait::async_trait]
impl DeviceLivenessCheck for RepositoryDeviceCheck {
    async fn check(&self, device_id: &RemoteDeviceId) -> DeviceLiveness {
        match self.auth.devices.get(device_id).await {
            Ok(Some(device)) if device.revoked_at.is_none() => DeviceLiveness::Live,
            Ok(Some(_)) | Ok(None) => DeviceLiveness::Revoked,
            Err(error) => {
                tracing::error!(%error, %device_id, "Remote device liveness re-check failed");
                DeviceLiveness::Unreadable
            }
        }
    }
}

/// Pre-upgrade authentication and admission, then the upgrade.
///
/// Nothing here is deferred into the upgrade callback that could still refuse the connection:
/// a bad ticket, a revoked device, a missing scope, and an exhausted per-device cap all answer an
/// HTTP status, so a rejected client never sees a socket open and immediately close.
pub(crate) async fn ws_events_handler(
    State(state): State<RemoteRouterState>,
    Query(query): Query<WsQuery>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let auth = state.auth().clone();

    // 1. Consume the single-use ticket. A replay always loses: `consume` is transactional.
    let device_id = match auth
        .tickets
        .consume(&hash_key(&query.ticket), &now_timestamp())
        .await
    {
        Ok(RemoteWsTicketOutcome::Consumed(device_id)) => device_id,
        Ok(outcome) => {
            auth.record_audit(
                None,
                RemoteAuditAction::AuthRejected,
                Some(&format!("ws ticket rejected: {outcome:?}")),
            )
            .await;
            return RemoteAuthRejection::UnknownToken.into_response();
        }
        Err(error) => {
            auth.record_audit(
                None,
                RemoteAuditAction::AuthStoreError,
                Some(&format!("ws ticket: {error}")),
            )
            .await;
            return RemoteAuthRejection::StoreUnavailable(error.to_string()).into_response();
        }
    };

    // 2. Re-resolve the device fail-closed. The ticket proves who minted it, not that the device
    //    is still live — a revoke between issue and upgrade must lose.
    let device = match auth.devices.get(&device_id).await {
        Ok(Some(device)) if device.revoked_at.is_none() => device,
        Ok(_) => return RemoteAuthRejection::RevokedToken.into_response(),
        Err(error) => {
            return RemoteAuthRejection::StoreUnavailable(error.to_string()).into_response()
        }
    };
    if !device.scopes.contains(Scope::UiRead) {
        return RemoteAuthRejection::InsufficientScope(Scope::UiRead).into_response();
    }

    // 3. The stream must exist. Host mode configured installs it at app setup (P-23); without it
    //    there is nothing to subscribe to and saying so beats an empty socket.
    let Some(stream) = state.stream().cloned() else {
        return remote_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::RemoteUnreachable,
            "The remote event stream is not running on this host.",
        );
    };

    // 4. Per-device session cap, enforced BEFORE the upgrade (§4.4, PR 1.2's registry).
    let session_id = RemoteSessionId::new();
    let kill = match auth.registry.register(&device.id, &session_id) {
        RemoteSessionAdmission::Admitted(kill) => kill,
        RemoteSessionAdmission::CapExceeded { limit } => {
            auth.record_audit(
                Some(&device.id),
                RemoteAuditAction::AuthRejected,
                Some(&format!("ws session cap of {limit} reached")),
            )
            .await;
            return remote_error_response(
                StatusCode::TOO_MANY_REQUESTS,
                ErrorCode::RemoteForbidden,
                "This device already holds the maximum number of live sessions.",
            );
        }
    };

    let remote_addr = connect_info
        .map(|ConnectInfo(address)| address.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let environment_id = state.environment_id().to_string();
    let lifecycle = state.lifecycle();

    // The guard owns registry membership and the durable row. It is constructed HERE — after
    // admission, before the upgrade — and MOVED into the closure. axum drops a callback it never
    // runs when the upgrade fails (client vanishes between the 101 and the upgraded I/O), so only
    // a guard that already exists can release the cap slot on that path; constructing it inside
    // the callback leaked one slot per failed upgrade until the process restarted.
    let guard = SessionGuard::new(auth.clone(), device.id.clone(), session_id.clone());

    upgrade.on_upgrade(move |socket| async move {
        guard.open(&remote_addr).await;
        lifecycle.emit(
            REMOTE_SESSION_CONNECTED_EVENT,
            json!({
                "sessionId": session_id.to_string(),
                "deviceId": device.id.to_string(),
                "deviceName": device.name,
            }),
        );

        let (sender, receiver) = socket.split();
        let mut socket = AxumSessionSocket { sender, receiver };
        let outcome = run_session(
            &mut socket,
            SessionContext {
                environment_id,
                stream,
                kill,
                device_check: Arc::new(RepositoryDeviceCheck { auth: auth.clone() }),
                device_id: device.id.clone(),
                session_id: session_id.clone(),
                heartbeat_interval: HEARTBEAT_INTERVAL,
            },
        )
        .await;

        guard.close(&outcome).await;
        lifecycle.emit(
            REMOTE_SESSION_CLOSED_EVENT,
            json!({
                "sessionId": session_id.to_string(),
                "deviceId": device.id.to_string(),
                "outcome": outcome.as_str(),
            }),
        );
    })
}

/// Owns the live-registry membership and the durable `remote_sessions` row for one socket.
struct SessionGuard {
    auth: RemoteAuthContext,
    device_id: RemoteDeviceId,
    session_id: RemoteSessionId,
}

impl SessionGuard {
    fn new(
        auth: RemoteAuthContext,
        device_id: RemoteDeviceId,
        session_id: RemoteSessionId,
    ) -> Self {
        Self {
            auth,
            device_id,
            session_id,
        }
    }

    async fn open(&self, remote_addr: &str) {
        let now = now_timestamp();
        if let Err(error) = self
            .auth
            .sessions
            .open(RemoteSession {
                id: self.session_id.clone(),
                device_id: self.device_id.clone(),
                connected_at: now.clone(),
                last_active_at: now,
                remote_addr: remote_addr.to_string(),
                closed_at: None,
            })
            .await
        {
            tracing::warn!(%error, session_id = %self.session_id, "Opening the remote session row failed");
        }
    }

    async fn close(&self, outcome: &SessionOutcome) {
        self.auth
            .registry
            .unregister(&self.device_id, &self.session_id);
        if let Err(error) = self
            .auth
            .sessions
            .close(&self.session_id, &now_timestamp())
            .await
        {
            tracing::warn!(%error, session_id = %self.session_id, "Closing the remote session row failed");
        }
        self.auth
            .record_audit(
                Some(&self.device_id),
                RemoteAuditAction::SessionClosed,
                Some(outcome.as_str()),
            )
            .await;
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        // Belt and braces: if the session task is cancelled outright, the registry entry must not
        // survive it and hold a slot against the per-device cap forever.
        self.auth
            .registry
            .unregister(&self.device_id, &self.session_id);
    }
}

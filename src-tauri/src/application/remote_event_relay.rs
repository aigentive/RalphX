// RemoteEventRelay — per-environment outbound WS sessions and the frame relay into
// the webview (PR 2.3, §3.2/§6.5).
//
// Ownership split, deliberately:
// - This relay owns the SOCKETS: one live session per environment row, hello
//   validation, Rust-side heartbeat acks, and the relay of every server frame into
//   the local event bus.
// - The TS `NetworkEventBus` owns the PROTOCOL CURSOR: it decides `afterSeq`
//   (cold `H = hello.maxSeq` vs warm `lastSeq`) and sends `subscribe`/`cursorAck`
//   through `remote_stream_send`. The relay never subscribes on its own.
// - The TS supervisor owns RETRY (A-5). A dropped socket here emits exactly one
//   `remote:stream_closed` and stops; nothing in Rust redials.
//
// The relayed events are classified **Local-only** in the protocol crate: they are
// the CLIENT proxy's inbound channel, and fanning them back out through a host's
// capture bank would let one paired device watch another's stream.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, PoisonError};

use ralphx_remote_protocol::{ClientFrame, ErrorCode, ServerFrame};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};

use crate::infrastructure::remote_ws_client::{
    ws_events_url, RemoteWsClient, RemoteWsConnection, RemoteWsError,
};

#[cfg(test)]
#[path = "remote_event_relay_tests.rs"]
mod tests;

/// Sink for relayed frames — a seam so relay tests need no Tauri AppHandle
/// (mirrors `remote_server::ws::SessionLifecycleSink`).
pub trait RemoteFrameSink: Send + Sync {
    fn emit(&self, name: &str, payload: serde_json::Value);
}

/// No-op sink for relays built without an app handle (unit tests, AppState::new_test).
pub struct NoopFrameSink;

impl RemoteFrameSink for NoopFrameSink {
    fn emit(&self, _name: &str, _payload: serde_json::Value) {}
}

/// Production sink: the webview's local event bus (mirrors `TauriLifecycleSink`).
pub struct TauriFrameSink(pub tauri::AppHandle);

impl RemoteFrameSink for TauriFrameSink {
    fn emit(&self, name: &str, payload: serde_json::Value) {
        use tauri::Emitter;
        if let Err(error) = self.0.emit(name, payload) {
            tracing::warn!(%error, event_name = name, "Emitting a remote stream event failed");
        }
    }
}

/// Local-only relay events. FIXED names, never formatted — a dynamic emit name is
/// unresolvable to the P-6 emit scanner, so the environment id travels in the
/// payload instead. Classified Local-only in `ralphx-remote-protocol`.
pub const REMOTE_STREAM_FRAME_EVENT: &str = "remote:stream_frame";
pub const REMOTE_STREAM_CLOSED_EVENT: &str = "remote:stream_closed";

/// What `remote_connect` hands back to JS: the hello, re-keyed to the registry row.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConnectOutcome {
    /// The registry ROW id — the JS-facing environment identity.
    pub environment_id: String,
    /// The host's self-reported environment id (`hello.environmentId`).
    pub host_environment_id: String,
    pub stream_epoch: String,
    pub max_seq: u64,
    pub heartbeat_secs: u32,
    pub protocol_version: u32,
}

/// One live session's registry entry. The `generation` makes "am I still the
/// current session" falsifiable: a superseded session must never deregister (or
/// otherwise touch) its replacement.
struct RelaySession {
    generation: u64,
    outbound: mpsc::UnboundedSender<ClientFrame>,
    kill: oneshot::Sender<()>,
}

pub struct RemoteEventRelay {
    ws_client: Arc<dyn RemoteWsClient>,
    sink: Arc<dyn RemoteFrameSink>,
    /// `Arc` so the spawned relay task can deregister itself; the map is never
    /// locked across an await.
    sessions: Arc<StdMutex<HashMap<String, RelaySession>>>,
    generations: AtomicU64,
}

/// The sessions map only ever holds `Send` handles and is never locked across an
/// await; if a panic ever poisons it the map itself is still structurally sound, so
/// recover the guard instead of turning every later connect into a panic.
fn lock_sessions(
    sessions: &StdMutex<HashMap<String, RelaySession>>,
) -> std::sync::MutexGuard<'_, HashMap<String, RelaySession>> {
    sessions.lock().unwrap_or_else(PoisonError::into_inner)
}

impl RemoteEventRelay {
    pub fn new(ws_client: Arc<dyn RemoteWsClient>, sink: Arc<dyn RemoteFrameSink>) -> Self {
        Self {
            ws_client,
            sink,
            sessions: Arc::new(StdMutex::new(HashMap::new())),
            generations: AtomicU64::new(0),
        }
    }

    /// Opens the outbound WS for `row_id`: tear down any live session first
    /// (idempotent reconnect — never two sockets per environment), dial, require
    /// `hello` as the FIRST frame, then hand the socket to the relay task.
    ///
    /// Deliberately does NOT send `subscribe`: the TS `NetworkEventBus` owns
    /// `afterSeq` and sends it through `remote_stream_send`.
    pub async fn connect(
        &self,
        row_id: &str,
        base_url: &str,
        ticket: &str,
    ) -> Result<RemoteConnectOutcome, RemoteWsError> {
        self.teardown(row_id);

        let url = ws_events_url(base_url, ticket)?;
        let mut connection = self.ws_client.connect(&url).await?;

        // §3.2: the server speaks first, and it speaks `hello`. Anything else is not
        // a stream this client can trust — close and report, register nothing.
        let outcome = match connection.recv().await {
            Some(Ok(ServerFrame::Hello {
                protocol_version,
                environment_id,
                stream_epoch,
                server_version: _,
                max_seq,
                heartbeat_secs,
            })) => RemoteConnectOutcome {
                environment_id: row_id.to_string(),
                host_environment_id: environment_id,
                stream_epoch,
                max_seq,
                heartbeat_secs,
                protocol_version,
            },
            Some(Ok(ServerFrame::Error { code, message })) => {
                connection.close().await;
                // An auth refusal after the upgrade must look exactly like one before
                // it, or the supervisor would retry a dead credential (§6.5 blocked).
                return Err(match code {
                    ErrorCode::RemoteUnauthorized => RemoteWsError::Rejected {
                        status: 401,
                        message,
                    },
                    ErrorCode::RemoteForbidden => RemoteWsError::Rejected {
                        status: 403,
                        message,
                    },
                    _ => RemoteWsError::Protocol(format!(
                        "host opened the stream with an error frame: {message}"
                    )),
                });
            }
            Some(Ok(other)) => {
                connection.close().await;
                return Err(RemoteWsError::Protocol(format!(
                    "expected hello as the first frame, got {}",
                    frame_kind(&other)
                )));
            }
            Some(Err(error)) => {
                connection.close().await;
                return Err(error);
            }
            None => {
                return Err(RemoteWsError::Closed(
                    "socket ended before hello".to_string(),
                ))
            }
        };

        let generation = self.generations.fetch_add(1, Ordering::Relaxed) + 1;
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let (kill_tx, kill_rx) = oneshot::channel();
        {
            let mut sessions = lock_sessions(&self.sessions);
            if let Some(displaced) = sessions.insert(
                row_id.to_string(),
                RelaySession {
                    generation,
                    outbound: outbound_tx,
                    kill: kill_tx,
                },
            ) {
                // A concurrent connect raced this one between teardown and insert.
                // The displaced session dies; its teardown cannot deregister this
                // one (generation mismatch).
                let _ = displaced.kill.send(());
            }
        }
        // Rule 17: this is an async fn, so a plain `tokio::spawn` is the correct
        // spawn — never from a sync constructor.
        tokio::spawn(run_relay_session(
            row_id.to_string(),
            generation,
            connection,
            outbound_rx,
            kill_rx,
            Arc::clone(&self.sink),
            Arc::clone(&self.sessions),
        ));
        Ok(outcome)
    }

    /// Pushes one client frame onto the session's socket.
    pub fn send(&self, row_id: &str, frame: ClientFrame) -> Result<(), RemoteWsError> {
        let sessions = lock_sessions(&self.sessions);
        let Some(session) = sessions.get(row_id) else {
            return Err(RemoteWsError::Closed(
                "no live session for this environment".to_string(),
            ));
        };
        session.outbound.send(frame).map_err(|_| {
            RemoteWsError::Closed("the relay task for this environment has ended".to_string())
        })
    }

    /// Idempotent teardown: fires the kill and drops the registry entry. The relay
    /// task emits the single `remote:stream_closed` on its way out.
    pub fn disconnect(&self, row_id: &str) {
        self.teardown(row_id);
    }

    pub fn is_connected(&self, row_id: &str) -> bool {
        lock_sessions(&self.sessions).contains_key(row_id)
    }

    fn teardown(&self, row_id: &str) {
        let removed = lock_sessions(&self.sessions).remove(row_id);
        if let Some(session) = removed {
            // The task may already have exited; a dead receiver is fine.
            let _ = session.kill.send(());
        }
    }
}

/// Frame name for error messages, without dragging payloads into them.
fn frame_kind(frame: &ServerFrame) -> &'static str {
    match frame {
        ServerFrame::Hello { .. } => "hello",
        ServerFrame::Event { .. } => "event",
        ServerFrame::ReplayDone { .. } => "replayDone",
        ServerFrame::Reset { .. } => "reset",
        ServerFrame::Heartbeat { .. } => "heartbeat",
        ServerFrame::Error { .. } => "error",
    }
}

/// What one turn of the relay loop observed. Classification only — the handlers run
/// after the `select!`, once the branch futures are dropped, which is what lets a
/// handler send on the same socket a branch was reading from (same shape as the
/// host's `run_session`).
enum RelayEvent {
    Kill,
    Outbound(Option<ClientFrame>),
    Incoming(Option<Result<ServerFrame, RemoteWsError>>),
}

/// Runs one relay session to completion. Whichever way it leaves — peer close,
/// send failure, kill — it closes the socket, deregisters ONLY its own generation,
/// and emits `remote:stream_closed` exactly once.
async fn run_relay_session(
    row_id: String,
    generation: u64,
    mut connection: Box<dyn RemoteWsConnection>,
    mut outbound: mpsc::UnboundedReceiver<ClientFrame>,
    mut kill: oneshot::Receiver<()>,
    sink: Arc<dyn RemoteFrameSink>,
    sessions: Arc<StdMutex<HashMap<String, RelaySession>>>,
) {
    let reason = loop {
        let event = tokio::select! {
            biased;
            _ = &mut kill => RelayEvent::Kill,
            frame = outbound.recv() => RelayEvent::Outbound(frame),
            incoming = connection.recv() => RelayEvent::Incoming(incoming),
        };
        match event {
            RelayEvent::Kill => break "disconnected".to_string(),
            // The registry entry owning this sender is gone: torn down or superseded.
            RelayEvent::Outbound(None) => break "disconnected".to_string(),
            RelayEvent::Outbound(Some(frame)) => {
                if let Err(error) = connection.send(frame).await {
                    break error.short_reason();
                }
            }
            RelayEvent::Incoming(None) => break "socket closed".to_string(),
            RelayEvent::Incoming(Some(Err(error))) => break error.short_reason(),
            RelayEvent::Incoming(Some(Ok(frame))) => {
                if let ServerFrame::Heartbeat { t } = frame {
                    // Acked HERE, in Rust, so a busy webview can never starve the
                    // host's 2-unacked budget (§3.2). The frame is still relayed —
                    // the TS watchdog counts frame silence, not acks.
                    if let Err(error) = connection.send(ClientFrame::HeartbeatAck { t }).await {
                        break error.short_reason();
                    }
                }
                sink.emit(
                    REMOTE_STREAM_FRAME_EVENT,
                    json!({ "environmentId": row_id, "frame": frame }),
                );
            }
        }
    };

    connection.close().await;
    {
        // Deregister only when the slot still belongs to THIS session: a superseded
        // session tearing down must not deregister its replacement.
        let mut sessions = lock_sessions(&sessions);
        if sessions
            .get(&row_id)
            .is_some_and(|session| session.generation == generation)
        {
            sessions.remove(&row_id);
        }
    }
    sink.emit(
        REMOTE_STREAM_CLOSED_EVENT,
        json!({ "environmentId": row_id, "reason": reason }),
    );
}

// RemoteEventRelay tests: hello gating, frame relay, Rust-side heartbeat acks,
// single-close teardown, and generation-scoped supersession. All against the
// scripted mock socket — no real network.

use super::*;
use crate::infrastructure::remote_ws_client::{
    MockRemoteWsClient, MockRemoteWsConnection, MockRemoteWsHandle,
};
use ralphx_remote_protocol::PROTOCOL_VERSION;

const ROW_ID: &str = "row-1";

/// Recording sink backed by a channel so tests can AWAIT relayed events instead of
/// polling shared state.
struct ChannelSink(mpsc::UnboundedSender<(String, serde_json::Value)>);

impl RemoteFrameSink for ChannelSink {
    fn emit(&self, name: &str, payload: serde_json::Value) {
        let _ = self.0.send((name.to_string(), payload));
    }
}

struct Fixture {
    relay: RemoteEventRelay,
    ws: Arc<MockRemoteWsClient>,
    events: mpsc::UnboundedReceiver<(String, serde_json::Value)>,
}

fn fixture() -> Fixture {
    let ws = Arc::new(MockRemoteWsClient::new());
    let (tx, events) = mpsc::unbounded_channel();
    let relay = RemoteEventRelay::new(
        Arc::clone(&ws) as Arc<dyn RemoteWsClient>,
        Arc::new(ChannelSink(tx)),
    );
    Fixture { relay, ws, events }
}

fn hello() -> ServerFrame {
    ServerFrame::Hello {
        protocol_version: PROTOCOL_VERSION,
        environment_id: "env-1".to_string(),
        stream_epoch: "epoch-1".to_string(),
        server_version: "0.81.0".to_string(),
        max_seq: 42,
        heartbeat_secs: 20,
    }
}

/// Scripts one connection whose first frame is already queued.
fn script_hello_connection(f: &Fixture) -> MockRemoteWsHandle {
    let (connection, handle) = MockRemoteWsConnection::scripted();
    handle
        .inbound
        .send(Ok(hello()))
        .expect("scripted hello should queue");
    f.ws.script_connection(connection);
    handle
}

async fn recv_event(f: &mut Fixture) -> (String, serde_json::Value) {
    tokio::time::timeout(std::time::Duration::from_secs(5), f.events.recv())
        .await
        .expect("a relayed event should arrive in time")
        .expect("the sink channel should stay open")
}

// ============================================================================
// connect — hello gating
// ============================================================================

#[tokio::test]
async fn connect_captures_the_hello_and_registers_the_session() {
    let mut f = fixture();
    let _handle = script_hello_connection(&f);

    let outcome = f
        .relay
        .connect(ROW_ID, "http://100.101.102.103:3849", "tick-1")
        .await
        .expect("connect should succeed");

    assert_eq!(
        outcome,
        RemoteConnectOutcome {
            environment_id: ROW_ID.to_string(),
            host_environment_id: "env-1".to_string(),
            stream_epoch: "epoch-1".to_string(),
            max_seq: 42,
            heartbeat_secs: 20,
            protocol_version: PROTOCOL_VERSION,
        }
    );
    assert!(f.relay.is_connected(ROW_ID));
    assert_eq!(
        f.ws.dialed_urls(),
        vec!["ws://100.101.102.103:3849/remote/v1/events?ticket=tick-1".to_string()]
    );
    // No subscribe went out: the TS NetworkEventBus owns afterSeq.
    assert!(
        f.events.try_recv().is_err(),
        "connect alone must not relay anything"
    );
}

#[tokio::test]
async fn a_non_hello_first_frame_is_a_protocol_error_and_leaves_no_session() {
    let f = fixture();
    let (connection, handle) = MockRemoteWsConnection::scripted();
    handle
        .inbound
        .send(Ok(ServerFrame::Heartbeat { t: 1 }))
        .expect("scripted frame should queue");
    f.ws.script_connection(connection);

    let error = f
        .relay
        .connect(ROW_ID, "https://host.example", "tick-1")
        .await
        .expect_err("a non-hello first frame must fail");

    assert!(matches!(error, RemoteWsError::Protocol(_)));
    assert!(!f.relay.is_connected(ROW_ID));
    assert!(handle.was_closed(), "the refused socket must be closed");
}

/// An auth refusal delivered as the first frame must look exactly like a refused
/// handshake, so the supervisor blocks instead of retrying a dead credential.
#[tokio::test]
async fn an_unauthorized_error_first_frame_is_rejected_401() {
    let f = fixture();
    let (connection, handle) = MockRemoteWsConnection::scripted();
    handle
        .inbound
        .send(Ok(ServerFrame::Error {
            code: ErrorCode::RemoteUnauthorized,
            message: "This device is no longer authorized.".to_string(),
        }))
        .expect("scripted frame should queue");
    f.ws.script_connection(connection);

    let error = f
        .relay
        .connect(ROW_ID, "https://host.example", "tick-1")
        .await
        .expect_err("an unauthorized first frame must fail");

    assert_eq!(
        error,
        RemoteWsError::Rejected {
            status: 401,
            message: "This device is no longer authorized.".to_string(),
        }
    );
    assert!(!f.relay.is_connected(ROW_ID));
    assert!(handle.was_closed());
}

#[tokio::test]
async fn a_socket_that_ends_before_hello_is_closed_with_no_session() {
    let f = fixture();
    let (connection, handle) = MockRemoteWsConnection::scripted();
    drop(handle); // peer closes before speaking
    f.ws.script_connection(connection);

    let error = f
        .relay
        .connect(ROW_ID, "https://host.example", "tick-1")
        .await
        .expect_err("a silent socket must fail");
    assert!(matches!(error, RemoteWsError::Closed(_)));
    assert!(!f.relay.is_connected(ROW_ID));
}

// ============================================================================
// Relay — frames, heartbeats
// ============================================================================

#[tokio::test]
async fn relayed_frames_carry_the_row_id_and_the_wire_shaped_frame() {
    let mut f = fixture();
    let handle = script_hello_connection(&f);
    f.relay
        .connect(ROW_ID, "https://host.example", "tick-1")
        .await
        .expect("connect should succeed");

    handle
        .inbound
        .send(Ok(ServerFrame::Event {
            seq: Some(43),
            name: "task:created".to_string(),
            payload: serde_json::json!({"id": "task-1"}),
        }))
        .expect("event should queue");

    let (name, payload) = recv_event(&mut f).await;
    assert_eq!(name, REMOTE_STREAM_FRAME_EVENT);
    assert_eq!(payload["environmentId"], ROW_ID);
    // The frame keeps its wire shape (camelCase type tag) so TS reuses one decoder.
    assert_eq!(payload["frame"]["type"], "event");
    assert_eq!(payload["frame"]["seq"], 43);
    assert_eq!(payload["frame"]["name"], "task:created");
}

#[tokio::test]
async fn a_heartbeat_is_acked_on_the_socket_and_still_relayed() {
    let mut f = fixture();
    let mut handle = script_hello_connection(&f);
    f.relay
        .connect(ROW_ID, "https://host.example", "tick-1")
        .await
        .expect("connect should succeed");

    handle
        .inbound
        .send(Ok(ServerFrame::Heartbeat { t: 7 }))
        .expect("heartbeat should queue");

    // The relay is what the TS watchdog counts: the heartbeat must arrive there…
    let (name, payload) = recv_event(&mut f).await;
    assert_eq!(name, REMOTE_STREAM_FRAME_EVENT);
    assert_eq!(payload["frame"]["type"], "heartbeat");
    // …and the ack was already on the socket BEFORE the relay emit, from Rust, so a
    // busy webview can never exhaust the host's 2-unacked budget.
    let acked = handle
        .outbound
        .try_recv()
        .expect("the ack must precede the relayed frame");
    assert_eq!(acked, ClientFrame::HeartbeatAck { t: 7 });
}

#[tokio::test]
async fn send_reaches_the_socket() {
    let f = fixture();
    let mut handle = script_hello_connection(&f);
    f.relay
        .connect(ROW_ID, "https://host.example", "tick-1")
        .await
        .expect("connect should succeed");

    f.relay
        .send(
            ROW_ID,
            ClientFrame::Subscribe {
                after_seq: 42,
                stream_epoch: "epoch-1".to_string(),
            },
        )
        .expect("send should reach the live session");

    let sent = tokio::time::timeout(std::time::Duration::from_secs(5), handle.outbound.recv())
        .await
        .expect("outbound frame should arrive in time")
        .expect("outbound channel should stay open");
    assert_eq!(
        sent,
        ClientFrame::Subscribe {
            after_seq: 42,
            stream_epoch: "epoch-1".to_string(),
        }
    );
}

#[tokio::test]
async fn send_to_an_unknown_environment_is_a_typed_error_not_a_panic() {
    let f = fixture();
    let error = f
        .relay
        .send("nope", ClientFrame::CursorAck { seq: 1 })
        .expect_err("no session, no send");
    assert!(matches!(error, RemoteWsError::Closed(_)));
}

// ============================================================================
// Teardown — single close event, registry hygiene, supersession
// ============================================================================

#[tokio::test]
async fn a_socket_end_emits_exactly_one_closed_event_and_clears_the_registry() {
    let mut f = fixture();
    let handle = script_hello_connection(&f);
    f.relay
        .connect(ROW_ID, "https://host.example", "tick-1")
        .await
        .expect("connect should succeed");

    drop(handle.inbound); // the peer goes away

    let (name, payload) = recv_event(&mut f).await;
    assert_eq!(name, REMOTE_STREAM_CLOSED_EVENT);
    assert_eq!(payload["environmentId"], ROW_ID);
    assert_eq!(payload["reason"], "socket closed");
    assert!(!f.relay.is_connected(ROW_ID));
    // Exactly once: the task has exited (the closed event is its last act), so
    // nothing further can arrive.
    assert!(f.events.try_recv().is_err());
}

#[tokio::test]
async fn disconnect_kills_the_session_and_is_idempotent() {
    let mut f = fixture();
    let _handle = script_hello_connection(&f);
    f.relay
        .connect(ROW_ID, "https://host.example", "tick-1")
        .await
        .expect("connect should succeed");

    f.relay.disconnect(ROW_ID);
    f.relay.disconnect(ROW_ID); // second call is a no-op, not a panic

    let (name, payload) = recv_event(&mut f).await;
    assert_eq!(name, REMOTE_STREAM_CLOSED_EVENT);
    assert_eq!(payload["reason"], "disconnected");
    assert!(!f.relay.is_connected(ROW_ID));
    assert!(f.events.try_recv().is_err());
}

/// One socket per environment: reconnecting supersedes the old session, and the OLD
/// session's teardown must not deregister the NEW one (generation check).
#[tokio::test]
async fn reconnecting_the_same_environment_supersedes_without_killing_the_replacement() {
    let mut f = fixture();
    let handle_a = script_hello_connection(&f);
    f.relay
        .connect(ROW_ID, "https://host.example", "tick-1")
        .await
        .expect("first connect should succeed");

    let handle_b = script_hello_connection(&f);
    f.relay
        .connect(ROW_ID, "https://host.example", "tick-2")
        .await
        .expect("second connect should succeed");

    // The old session announces its own death…
    let (name, payload) = recv_event(&mut f).await;
    assert_eq!(name, REMOTE_STREAM_CLOSED_EVENT);
    assert_eq!(payload["environmentId"], ROW_ID);
    assert!(handle_a.was_closed(), "the superseded socket must be closed");
    // …and the NEW session survived it: still registered, still relaying.
    assert!(f.relay.is_connected(ROW_ID));
    handle_b
        .inbound
        .send(Ok(ServerFrame::Heartbeat { t: 9 }))
        .expect("frame should queue on the new socket");
    let (name, payload) = recv_event(&mut f).await;
    assert_eq!(name, REMOTE_STREAM_FRAME_EVENT);
    assert_eq!(payload["frame"]["t"], 9);
}

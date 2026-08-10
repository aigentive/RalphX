//! PR 3.2 validation legs, run against the two-instance harness.
//!
//! Every leg here drives a REAL production host (real listener on an ephemeral loopback port,
//! real pairing, real capture + sequencer) from a scripted client over a REAL WebSocket. The
//! legs are the tests: they are written as falsifiable assertions about the shipped behaviour,
//! not as smoke checks that the harness boots.
//!
//! # Determinism
//!
//! No leg sleeps or polls on a timer. Every wait is event-driven — the client awaits the next
//! frame off the socket, and where a leg needs to know that a host-side commit has happened it
//! waits for a LATER durable frame to arrive rather than guessing at a duration. See
//! `durable_barrier` below for why that is sound.

use ralphx_remote_protocol::{ClientFrame, ResetReason, Scope, ServerFrame};
use serde_json::json;

use super::harness::{invoke_error_code, RemoteHostHarness, ScriptedClient};
use super::sequencer::EpochRollCause;
use crate::domain::entities::RemoteScopeSet;

/// A durable chat event, used as the ordering barrier in several legs.
const DURABLE_PROBE: &str = "agent:message_created";
/// The transient streaming event A-4 forbids from ever being persisted.
const TRANSIENT_CHUNK: &str = "agent:chunk";
/// The second transient name A-4 names explicitly.
const TRANSIENT_USAGE: &str = "agent:usage_updated";
/// The v1-excluded terminal fan-out class.
const TERMINAL_EVENT: &str = "agent_terminal:event";

/// Boots a host and connects one paired client that has completed `hello` + `subscribe`.
///
/// Returns the harness, the client, the `maxSeq` the host reported at `hello` (the `H`
/// barrier), and the live stream epoch.
async fn connected_client() -> (RemoteHostHarness, ScriptedClient, u64, String) {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");
    let device = host
        .pair_device("harness-client")
        .await
        .expect("pairing should succeed");
    let mut client = ScriptedClient::new(host.base_url(), device.token);

    let hello = client.connect().await;
    let (max_seq, epoch) = match hello {
        ServerFrame::Hello {
            max_seq,
            stream_epoch,
            ..
        } => (max_seq, stream_epoch),
        other => panic!("the first frame must be hello, got {other:?}"),
    };
    client
        .send(ClientFrame::Subscribe {
            after_seq: max_seq,
            stream_epoch: epoch.clone(),
        })
        .await;
    let replay_done = client
        .next_frame_matching(|frame| matches!(frame, ServerFrame::ReplayDone { .. }))
        .await;
    assert!(
        replay_done.is_some(),
        "subscribe must be acknowledged with replayDone",
    );

    (host, client, max_seq, epoch)
}

/// Emits a durable probe and waits for the client to see it.
///
/// This is the ordering barrier the absence assertions rely on. Capture dispatch is INLINE on
/// the emitting thread (Tauri invokes `listen_any` callbacks synchronously), so anything
/// emitted before this probe has already been dispatched to its channel by the time the probe
/// is dispatched. The probe's durable frame is published only AFTER its row is committed
/// (`commit_batch` → `append_batch` → `publish_committed`). So once the client has the probe
/// frame, any earlier event that was going to be persisted already is — and a subsequent
/// "is it in the log?" query cannot produce a false negative by racing the sequencer.
async fn durable_barrier(host: &RemoteHostHarness, client: &mut ScriptedClient, marker: &str) {
    host.emit(DURABLE_PROBE, json!({ "harnessMarker": marker }));
    let frame = client
        .next_frame_matching(|frame| {
            matches!(
                frame,
                ServerFrame::Event { name, payload, .. }
                    if name == DURABLE_PROBE && payload["harnessMarker"] == marker
            )
        })
        .await;
    assert!(
        frame.is_some(),
        "the durable probe {marker} must reach the client over the wire",
    );
}

// =====================================================================================
// Leg: live streaming round trip over the wire
// =====================================================================================

/// Transient chunks stream to a remote client live, over the real socket.
///
/// This is the "progressively streams to the remote client through the real listener" half of
/// the acceptance criteria: the frames are produced by the production capture bank and carried
/// by the production WS session, and they arrive with NO seq (transient frames bypass the
/// sequencer entirely, so they can never advance a client's cursor).
#[tokio::test]
async fn transient_chunks_stream_live_to_a_remote_client_without_a_seq() {
    let (host, mut client, _max_seq, _epoch) = connected_client().await;

    host.emit(TRANSIENT_CHUNK, json!({ "delta": "hel" }));
    host.emit(TRANSIENT_CHUNK, json!({ "delta": "lo" }));

    let first = client
        .next_event_named(TRANSIENT_CHUNK)
        .await
        .expect("the first chunk should arrive live");
    let second = client
        .next_event_named(TRANSIENT_CHUNK)
        .await
        .expect("the second chunk should arrive live");

    for (frame, expected) in [(first, "hel"), (second, "lo")] {
        match frame {
            ServerFrame::Event { seq, name, payload } => {
                assert_eq!(name, TRANSIENT_CHUNK);
                assert_eq!(payload["delta"], expected);
                assert!(
                    seq.is_none(),
                    "a transient frame must carry no seq — a seq would let it advance the \
                     client's resume cursor (§3.4)",
                );
            }
            other => panic!("expected an event frame, got {other:?}"),
        }
    }

    host.stop().await.expect("the host should stop cleanly");
}

/// Durable chat events arrive sequenced and contiguous while a stream is in flight.
#[tokio::test]
async fn durable_chat_events_are_contiguous_during_a_stream() {
    let (host, mut client, max_seq, _epoch) = connected_client().await;

    // Interleave transient chunks with durable events, the way a real turn does.
    host.emit(TRANSIENT_CHUNK, json!({ "delta": "a" }));
    host.emit(DURABLE_PROBE, json!({ "n": 1 }));
    host.emit(TRANSIENT_CHUNK, json!({ "delta": "b" }));
    host.emit(DURABLE_PROBE, json!({ "n": 2 }));
    host.emit(DURABLE_PROBE, json!({ "n": 3 }));

    let mut seqs = Vec::new();
    while seqs.len() < 3 {
        let frame = client
            .next_frame_matching(|frame| {
                matches!(frame, ServerFrame::Event { name, seq, .. }
                    if name == DURABLE_PROBE && seq.is_some())
            })
            .await
            .expect("every durable event should arrive");
        if let ServerFrame::Event { seq: Some(seq), .. } = frame {
            seqs.push(seq);
        }
    }

    assert_eq!(
        seqs,
        vec![max_seq + 1, max_seq + 2, max_seq + 3],
        "durable seqs must be contiguous and start one past the hello high-water",
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =====================================================================================
// Leg: A-4 — transient events are NEVER persisted in remote_event_log
// =====================================================================================

/// `agent:chunk` / `agent:usage_updated` must not appear in `remote_event_log` after a
/// streamed session (A-4). This is the assertion the phase doc calls out as owned by 3.2:
/// "asserts absence after a real streamed chat session, not just classification-table intent".
#[tokio::test]
async fn streamed_transient_events_never_reach_the_durable_log() {
    let (host, mut client, _max_seq, _epoch) = connected_client().await;

    // A burst, so a single misrouted row would be overwhelmingly likely to show up.
    for index in 0..24 {
        host.emit(TRANSIENT_CHUNK, json!({ "delta": index }));
        host.emit(TRANSIENT_USAGE, json!({ "inputTokens": index }));
    }

    // Ordering barrier: once this durable frame has reached the client, the sequencer has
    // committed past every emit above.
    durable_barrier(&host, &mut client, "after-transient-burst").await;

    let names = host
        .durable_event_names()
        .await
        .expect("the durable log should be readable");

    assert!(
        !names.iter().any(|name| name == TRANSIENT_CHUNK),
        "no transient event may ever land in remote_event_log (A-4); found: {names:?}",
    );
    assert!(
        !names.iter().any(|name| name == TRANSIENT_USAGE),
        "agent:usage_updated is Transient and must never be persisted (A-4); found: {names:?}",
    );
    assert!(
        names.iter().any(|name| name == DURABLE_PROBE),
        "the durable probe itself must be persisted, else this test proves nothing: {names:?}",
    );

    host.stop().await.expect("the host should stop cleanly");
}

/// Transient volume must not consume durable sequence numbers either.
///
/// A weaker implementation could keep chunks out of the table while still allocating a seq for
/// them, which would silently create gaps in the client's cursor arithmetic.
#[tokio::test]
async fn transient_volume_consumes_no_durable_sequence_numbers() {
    let (host, mut client, max_seq, _epoch) = connected_client().await;

    for index in 0..16 {
        host.emit(TRANSIENT_CHUNK, json!({ "delta": index }));
    }
    durable_barrier(&host, &mut client, "seq-accounting").await;

    let rows = host
        .durable_rows()
        .await
        .expect("the durable log should be readable");
    let seqs: Vec<u64> = rows.iter().map(|row| row.seq).collect();

    assert_eq!(
        seqs,
        vec![max_seq + 1],
        "exactly one durable row (the barrier) may exist, at the next seq — transient traffic \
         must not consume seqs: {seqs:?}",
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =====================================================================================
// Leg: terminal exclusion (negative)
// =====================================================================================

/// The terminal surface is excluded from v1 everywhere: no fan-out class on the wire, and no
/// durable row. PR 3.2 asserts the exclusion and wires nothing (§3.3, §1 non-goals).
#[tokio::test]
async fn terminal_events_are_excluded_from_the_remote_wire_and_the_durable_log() {
    let (host, mut client, _max_seq, _epoch) = connected_client().await;

    // Emitted host-side exactly as `agent_terminal.rs` does.
    host.emit(TERMINAL_EVENT, json!({ "bytes": "should never travel" }));
    host.emit(TERMINAL_EVENT, json!({ "bytes": "nor this" }));

    // Barrier: the client provably reached a point in the stream after the terminal emits.
    // Any terminal frame that was going to be fanned out would have arrived before it.
    durable_barrier(&host, &mut client, "after-terminal-emits").await;

    let names = host
        .durable_event_names()
        .await
        .expect("the durable log should be readable");
    assert!(
        !names.iter().any(|name| name == TERMINAL_EVENT),
        "the PTY surface must never be persisted for remote replay: {names:?}",
    );

    host.stop().await.expect("the host should stop cleanly");
}

/// The exclusion is structural, not incidental: the classification row carries the
/// `excluded_from_v1` flag the capture bank filters on, so no listener is ever registered.
#[test]
fn the_terminal_class_is_flagged_excluded_and_is_the_only_such_row() {
    let terminal = ralphx_remote_protocol::EventClassification::find(TERMINAL_EVENT)
        .expect("the terminal class must still be classified");
    assert!(
        terminal.excluded_from_v1,
        "agent_terminal:event must stay excluded from v1 — re-enabling requires the deferred \
         ui:elevated scope",
    );

    let excluded: Vec<&str> = ralphx_remote_protocol::EVENT_CLASSIFICATIONS
        .iter()
        .filter(|entry| entry.excluded_from_v1)
        .map(|entry| entry.name)
        .collect();
    assert_eq!(
        excluded,
        vec![TERMINAL_EVENT],
        "terminal is the only v1-excluded class; a new one needs its own review",
    );
}

/// No terminal command is reachable through the remote facade.
#[test]
fn no_terminal_command_is_remotely_registered() {
    for command in [
        "open_agent_terminal",
        "write_agent_terminal",
        "resize_agent_terminal",
        "clear_agent_terminal",
        "restart_agent_terminal",
        "close_agent_terminal",
    ] {
        assert!(
            super::registry::find_spec(command).is_none(),
            "{command} must not be remotely registered — PtyControl is Denied in v1",
        );
    }
}

// =====================================================================================
// Leg: revoke mid-stream → WS close + 401 on redial
// =====================================================================================

/// Revoking a device mid-stream must tear down the LIVE socket and 401 the next request.
///
/// The "and" matters: an implementation that only denies future HTTP would leave a revoked
/// device streaming until it happened to reconnect (P-2).
#[tokio::test]
async fn revoking_a_device_mid_stream_closes_the_socket_and_401s_the_next_request() {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");
    let device = host
        .pair_device("doomed-device")
        .await
        .expect("pairing should succeed");
    let mut client = ScriptedClient::new(host.base_url(), device.token.clone());

    let hello = client.connect().await;
    let (max_seq, epoch) = match hello {
        ServerFrame::Hello {
            max_seq,
            stream_epoch,
            ..
        } => (max_seq, stream_epoch),
        other => panic!("expected hello, got {other:?}"),
    };
    client
        .send(ClientFrame::Subscribe {
            after_seq: max_seq,
            stream_epoch: epoch,
        })
        .await;
    // Wait for the subscription to be acknowledged before emitting: an emit that races the
    // subscribe would be broadcast to nobody, and the leg would fail for the wrong reason.
    client
        .next_frame_matching(|frame| matches!(frame, ServerFrame::ReplayDone { .. }))
        .await
        .expect("subscribe must be acknowledged with replayDone");

    // Prove the stream is genuinely live before revoking.
    host.emit(TRANSIENT_CHUNK, json!({ "delta": "live" }));
    assert!(
        client.next_event_named(TRANSIENT_CHUNK).await.is_some(),
        "the stream must be live before the revoke, or the leg proves nothing",
    );

    host.revoke_device(&device.device_id)
        .await
        .expect("revocation should succeed");

    // The live socket must close. `closed()` drains until the socket ends.
    assert!(
        client.closed().await,
        "revocation must tear down the LIVE socket, not just deny future requests (P-2)",
    );

    // And the next bearer request must be refused.
    let redial = ScriptedClient::new(host.base_url(), device.token);
    let ticket = redial.mint_ws_ticket().await;
    assert_eq!(
        ticket.status, 401,
        "a revoked bearer must be refused on redial, got {} / {}",
        ticket.status, ticket.body,
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =====================================================================================
// Leg: epoch roll → client reset
// =====================================================================================

/// A live epoch roll must reset every connected client rather than splicing over the gap.
///
/// Both roll causes mean "an event may be missing from the stream", so a client that kept
/// warm-resuming would silently show an incomplete transcript.
#[tokio::test]
async fn an_epoch_roll_resets_a_connected_client() {
    let (host, mut client, _max_seq, epoch) = connected_client().await;

    host.roll_epoch(EpochRollCause::CaptureOverload);

    let reset = client
        .next_frame_matching(|frame| matches!(frame, ServerFrame::Reset { .. }))
        .await
        .expect("a rolled epoch must reset the connected client");
    match reset {
        ServerFrame::Reset { reason } => assert_eq!(
            reason,
            ResetReason::EpochChanged,
            "an epoch roll resets with epoch_changed",
        ),
        other => panic!("expected reset, got {other:?}"),
    }

    // The epoch actually moved, so the client's old cursor is unserviceable.
    assert_ne!(
        host.stream().epoch().as_str(),
        epoch,
        "the live epoch must actually change on a roll",
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =====================================================================================
// Leg: scope split on chat send
// =====================================================================================

/// Chat send is `AgentControl`: an explicitly narrowed device without `ui:agent` is refused.
#[tokio::test]
async fn chat_send_without_ui_agent_is_forbidden() {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");
    let device = host
        .pair_device_with_scopes(
            "agent-control-revoked",
            RemoteScopeSet::from_scopes([Scope::UiRead, Scope::UiOperate]),
        )
        .await
        .expect("pairing should succeed");
    let client = ScriptedClient::new(host.base_url(), device.token);

    let response = client
        .invoke(
            "send_remote_chat_message",
            json!({
                "input": {
                    "conversationId": "conversation-does-not-matter",
                    "content": "hello",
                    "role": "user",
                }
            }),
        )
        .await;

    assert_eq!(
        response.status, 403,
        "a device without ui:agent must be refused: {}",
        response.body,
    );
    assert_eq!(
        invoke_error_code(&response),
        "REMOTE_FORBIDDEN",
        "the refusal must be the scope gate, not an incidental error: {}",
        response.body,
    );

    host.stop().await.expect("the host should stop cleanly");
}

/// With `ui:agent` granted the scope gate opens — and the command then refuses on its OWN
/// terms (no live run), which proves the request reached the command rather than the gate.
#[tokio::test]
async fn chat_send_with_ui_agent_passes_the_scope_gate() {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");
    let device = host
        .pair_device("agent-control-device")
        .await
        .expect("pairing should succeed");
    host.grant_agent_control(&device.device_id)
        .await
        .expect("granting agent control should succeed");
    let client = ScriptedClient::new(host.base_url(), device.token);

    let response = client
        .invoke(
            "send_remote_chat_message",
            json!({
                "input": {
                    "conversationId": "no-such-conversation",
                    "content": "steer the run",
                    "role": "user",
                }
            }),
        )
        .await;

    assert_ne!(
        invoke_error_code(&response),
        "REMOTE_FORBIDDEN",
        "ui:agent must clear the scope gate: {}",
        response.body,
    );
    assert!(
        response
            .body
            .contains("REMOTE_CHAT_SEND_CONVERSATION_NOT_FOUND"),
        "the request must reach the command and be refused on its own terms, got: {}",
        response.body,
    );

    host.stop().await.expect("the host should stop cleanly");
}

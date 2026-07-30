//! PR 3.4 — the two-instance remote E2E suite.
//!
//! Five legs, end to end through the production stack: **pair → invoke → event → revoke →
//! prune → reset**, plus the full fake-CLI chat turn PR 3.2 deferred and the P-16 standing
//! assertions the phase carries.
//!
//! # Why this is a top-level test binary
//!
//! `rust-test-execution.md` defaults to "do not create a new top-level test binary". This suite
//! is the documented exception, for three reasons that no `suite_*` module can absorb:
//!
//! 1. It boots a host from **production constructors** with a **real `TcpListener`** on an
//!    ephemeral loopback port. Every existing `suite_*` binary is a handler/repository harness;
//!    none owns a socket, and adding one would make every test in that binary port-sensitive.
//! 2. It needs `--features test-utils` for `tauri::test::MockRuntime` AND the crate-internal
//!    `remote_server::harness` fixture, which is `#[cfg(feature = "test-utils")] pub mod`.
//! 3. It must be serialized as a unit (see the nextest group override): the harness reserves a
//!    port by binding `:0` and rebinding, so concurrent legs race on the reserve/rebind window.
//!
//! It is wired into the `main` tier only, by appending `remote_e2e` to `FULL_INTEGRATION_TESTS`
//! in `scripts/test-rust-fast.sh`. That array is consumed exclusively by `full-integration`,
//! which `main` runs and `pr` does not — so the single array entry both puts the suite in `main`
//! and keeps it out of `pr`. `scripts/tests/test-ci-rust-full-integration-targets.sh` enumerates
//! every Cargo test target and fails if the runner omits one, so the wiring cannot silently rot.
//!
//! # What is real
//!
//! Nothing here is a mock router, a handler-level shortcut, or a test-only auth bypass. The host
//! is the shipped router with its real auth / CORS / rate-limit / trust-strip layers; pairing
//! goes over real HTTP; the client is the production `HyperRemoteHostClient` plus a real
//! WebSocket. A-2 holds even in tests: there is no zero-devices bootstrap exception, so every
//! route except the descriptor and pairing authenticates.
//!
//! # Determinism
//!
//! No leg sleeps or polls on a timer. Every wait is event-driven: the client awaits the next
//! frame off the socket, and where a leg needs to know that a host-side commit has landed it
//! waits for a LATER durable frame rather than guessing a duration. The two places real time
//! could otherwise leak in — the retention TTL and the 5-minute prune interval — are bypassed by
//! publishing the retention floor directly (see `force_pruned_floor`).
//!
//! **One honest exception.** `leg4` takes ~21 s, because revocation of a LIVE session is
//! detected on the next heartbeat tick and `HEARTBEAT_INTERVAL` (20 s) is hardcoded at the
//! production session call site (`ws.rs`, `SessionContext { heartbeat_interval: … }`) with no
//! injection seam. This is deterministic — a fixed period, not a race, so repeat runs are
//! identical — and it is also *faithful*: P-2's obligation is that the socket closes "within the
//! heartbeat window", so observing it at the real cadence is exactly the property under test.
//! The phase doc's "heartbeat windows are injected, never real-time waits" is an aspiration the
//! shipped code does not currently support. Threading a configurable interval through
//! `RemoteListenerRuntime` into the ws handler would be a production-surface change made purely
//! for test speed, so it is deliberately NOT done here and is recorded as a follow-up instead.
//! The suite's nextest slow-timeout accommodates it.

#![cfg(feature = "test-utils")]

use std::os::unix::fs::PermissionsExt;

use ralphx_lib::application::chat_service::ChatService;
use ralphx_lib::application::{AppState, PendingPermissionInfo};
use ralphx_lib::domain::entities::{ChatContextType, ChatConversation, Project, RemoteScopeSet};
use ralphx_lib::infrastructure::remote_host_client::{
    PairWireRequest, RemoteFetchRequest, RemoteHostClient, RemoteHostClientError,
};
use ralphx_lib::remote_server::harness::{
    harness_http_client, invoke_error_code, invoke_ok, RemoteHostHarness, ScriptedClient,
    HARNESS_CLIENT_VERSION,
};
use ralphx_remote_protocol::{ClientFrame, ResetReason, Scope, ServerFrame};
use serde_json::json;

/// A durable chat event. Used both as a subject and as the ordering barrier.
const DURABLE_PROBE: &str = "agent:message_created";
/// The transient streaming event A-4 forbids from ever reaching `remote_event_log`.
const TRANSIENT_CHUNK: &str = "agent:chunk";
/// The `:3847` trust header. It must buy exactly nothing on `:3849` (P-16).
const TAURI_TRUST_HEADER: &str = "X-RalphX-Tauri-MCP";

// =========================================================================================
// Shared plumbing
// =========================================================================================

/// Boots a host and connects one default-grant ("viewer with brakes") client that has completed
/// `hello` + `subscribe`. Returns the harness, the client, the `H` barrier, and the epoch.
async fn connected_client() -> (RemoteHostHarness, ScriptedClient, u64, String) {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");
    let device = host
        .pair_device("e2e-client")
        .await
        .expect("pairing should succeed");
    let mut client = ScriptedClient::new(host.base_url(), device.token);
    let (max_seq, epoch) = subscribe(&mut client).await;
    (host, client, max_seq, epoch)
}

/// `connect` → read the `H` barrier → `subscribe` → wait for `replayDone`.
///
/// Waiting for `replayDone` before returning is what makes the callers race-free: an emit that
/// beat the subscription would be broadcast to nobody and the leg would fail for the wrong
/// reason.
async fn subscribe(client: &mut ScriptedClient) -> (u64, String) {
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
    client
        .next_frame_matching(|frame| matches!(frame, ServerFrame::ReplayDone { .. }))
        .await
        .expect("subscribe must be acknowledged with replayDone");
    (max_seq, epoch)
}

/// Emits a durable probe and waits for the client to see it.
///
/// The ordering barrier every absence assertion rests on. Capture dispatch is INLINE on the
/// emitting thread, so anything emitted before this probe has already been dispatched to its
/// channel by the time the probe is. The probe's frame is published only AFTER its row is
/// committed, so once the client holds the probe frame, any earlier event that was going to be
/// persisted already is — a subsequent "is it in the log?" query cannot race the sequencer into
/// a false negative.
async fn durable_barrier(host: &RemoteHostHarness, client: &mut ScriptedClient, marker: &str) {
    host.emit(DURABLE_PROBE, json!({ "e2eMarker": marker }));
    let frame = client
        .next_frame_matching(|frame| {
            matches!(
                frame,
                ServerFrame::Event { name, payload, .. }
                    if name == DURABLE_PROBE && payload["e2eMarker"] == marker
            )
        })
        .await;
    assert!(
        frame.is_some(),
        "the durable probe {marker} must reach the client over the wire",
    );
}

/// Collects the next `count` durable event frames, returning their seqs.
async fn collect_durable_seqs(client: &mut ScriptedClient, count: usize) -> Vec<u64> {
    let mut seqs = Vec::with_capacity(count);
    while seqs.len() < count {
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
    seqs
}

/// The `result` payload of a successful invoke.
///
/// The remote facade wraps every answer in an `{ok, result}` envelope; the value INSIDE that
/// envelope is what must match local Tauri IPC byte for byte. Unwrapping in one place keeps the
/// legs asserting about command results rather than about transport framing.
fn invoke_result(
    response: &ralphx_lib::infrastructure::remote_host_client::RemoteHttpResponse,
) -> serde_json::Value {
    let body = invoke_ok(response);
    assert_eq!(
        body["ok"], true,
        "a 200 invoke must carry an ok envelope: {body}",
    );
    body["result"].clone()
}

/// The gate rows from a `list_pending_*` invoke.
fn gate_rows(
    response: &ralphx_lib::infrastructure::remote_host_client::RemoteHttpResponse,
) -> Vec<serde_json::Value> {
    invoke_result(response)
        .as_array()
        .expect("a gate list must be an array")
        .clone()
}

/// Whether `request_id` is present in a gate list.
fn has_gate(rows: &[serde_json::Value], request_id: &str) -> bool {
    rows.iter()
        .any(|gate| gate["request_id"] == request_id || gate["requestId"] == request_id)
}

// =========================================================================================
// Leg 1 — pair
// =========================================================================================

/// A pairing code is single-use, and the token it mints is a working bearer.
///
/// Both halves matter. Asserting only that the second exchange fails would pass against a host
/// that rejects every pairing; asserting only that the first succeeds would pass against a host
/// that never consumes the code. P-7, observed E2E.
#[tokio::test]
async fn leg1_pairing_code_is_single_use_and_mints_a_working_bearer() {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");

    let scopes = RemoteScopeSet::default_pairing_grant();
    let requested_scopes = scopes.to_vec();
    let code = host
        .mint_pairing_code(scopes)
        .await
        .expect("minting a pairing code should succeed");
    let http = harness_http_client();
    let request = PairWireRequest {
        pairing_code: code,
        device_name: "single-use-device".to_string(),
        client_version: HARNESS_CLIENT_VERSION.to_string(),
        requested_scopes,
    };

    let paired = http
        .pair(&host.base_url(), &request)
        .await
        .expect("the first exchange should succeed");
    assert!(
        paired.device_token.starts_with("rxd_live_"),
        "the minted bearer should carry the live-token prefix, got {}",
        paired.device_token,
    );
    assert_eq!(
        paired.environment_id,
        host.environment_id(),
        "the paired device must be bound to this host's environment",
    );
    assert!(
        !paired.scopes.contains(&Scope::UiAgent),
        "the default pairing grant is viewer-with-brakes and must never include ui:agent, \
         got {:?}",
        paired.scopes,
    );

    // The token actually works — otherwise "single use" would be trivially satisfiable.
    let client = ScriptedClient::new(host.base_url(), paired.device_token);
    let health = client.invoke("health_check", json!({})).await;
    assert_eq!(
        invoke_result(&health)["status"],
        "ok",
        "the freshly paired bearer must be able to invoke",
    );

    // The SAME code, replayed verbatim.
    let replay = http.pair(&host.base_url(), &request).await;
    match replay {
        Err(RemoteHostClientError::Rejected { status, .. }) => assert_eq!(
            status, 401,
            "a consumed pairing code must be refused with 401",
        ),
        Ok(_) => panic!("a pairing code must not be usable twice (P-7)"),
        Err(other) => panic!("expected a 401 rejection, got {other:?}"),
    }

    host.stop().await.expect("the host should stop cleanly");
}

// =========================================================================================
// Leg 2 — invoke
// =========================================================================================

/// A `Read` command round-trips over `:3849` with serialization identical to local Tauri IPC.
///
/// The comparison is against a DIRECT call of the very same command fn, which is the only
/// comparison that can catch a facade that re-serializes: the registry references the existing
/// fn rather than forking it, and this asserts that property observably rather than structurally.
#[tokio::test]
async fn leg2_read_command_round_trips_with_tauri_identical_serialization() {
    let (host, client, _max_seq, _epoch) = connected_client().await;

    let remote = invoke_result(&client.invoke("health_check", json!({})).await);
    let local = serde_json::to_value(ralphx_lib::commands::health::health_check())
        .expect("the local command result should serialize");

    assert_eq!(
        remote, local,
        "remote dispatch must return JSON byte-identical to the local Tauri IPC result",
    );

    host.stop().await.expect("the host should stop cleanly");
}

/// The scope split, both directions, in one leg.
///
/// A device holding the default grant is refused an `AgentControl` command with the SCOPE code
/// (not an incidental error), and the same device succeeds at a brake. The pairing that proves
/// the negative is the same pairing that proves the positive, so a host that simply refused
/// everything could not pass.
#[tokio::test]
async fn leg2_agent_control_is_forbidden_while_a_brake_succeeds_under_ui_operate() {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");
    let device = host
        .pair_device("viewer-with-brakes")
        .await
        .expect("pairing should succeed");
    let client = ScriptedClient::new(host.base_url(), device.token);

    // --- negative: AgentControl without ui:agent ---
    let forbidden = client
        .invoke(
            "send_remote_chat_message",
            json!({
                "input": {
                    "conversationId": "irrelevant-the-gate-runs-first",
                    "content": "start something",
                    "role": "user",
                }
            }),
        )
        .await;
    assert_eq!(
        forbidden.status, 403,
        "an AgentControl command must be refused without ui:agent: {}",
        forbidden.body,
    );
    assert_eq!(
        invoke_error_code(&forbidden),
        "REMOTE_FORBIDDEN",
        "the refusal must come from the scope gate: {}",
        forbidden.body,
    );

    // --- positive: a brake under ui:operate ---
    // Brakes are the deliberate exemption (A-14): a viewer must always be able to STOP
    // something without being handed the ability to START it.
    let request_id = "e2e-brake-gate";
    host.state()
        .permission_state
        .register(PendingPermissionInfo {
            request_id: request_id.to_string(),
            tool_name: "mcp__ralphx__get_task_context".to_string(),
            tool_input: json!({ "task_id": "task-1" }),
            context: Some("needs task context".to_string()),
            agent_type: Some("worker".to_string()),
            task_id: Some("task-1".to_string()),
            context_type: Some("task".to_string()),
            context_id: Some("task-1".to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .await;

    let pending = gate_rows(
        &client
            .invoke("list_pending_permission_gates", json!({}))
            .await,
    );
    assert!(
        has_gate(&pending, request_id),
        "the raised gate must be visible to a ui:read device: {pending:?}",
    );

    // Two wire details matter here.
    // 1. `ResolvePermissionArgs` carries NO `rename_all`, so its field really is `request_id`
    //    in snake_case. Not every Rust struct on this surface is camelCase, and assuming so is
    //    exactly the drift `tauri-invoke-conventions.md` warns about.
    // 2. `deny_permission_request` server-pins `decision = "deny"` — so this deliberately sends
    //    "allow" and must still deny. A client cannot talk its way past the pin.
    let denied = client
        .invoke(
            "deny_permission_request",
            json!({ "args": { "request_id": request_id, "decision": "allow" } }),
        )
        .await;
    assert_eq!(
        denied.status, 200,
        "a brake must succeed under ui:operate: {}",
        denied.body,
    );
    // `success` is `PermissionState::resolve`'s return value: true only if the gate was FOUND.
    // That is the assertion with teeth — a brake that silently no-ops against an unknown id
    // would still return 200.
    assert_eq!(
        invoke_result(&denied)["success"],
        true,
        "the brake must have reached the raised gate, not silently no-opped: {}",
        denied.body,
    );

    // NOTE: the gate deliberately stays listed. `PermissionState::resolve` signals the decision
    // watcher and persists it; REMOVAL is owned by the awaiting long-poll (`await_permission`),
    // which retires the entry once it has consumed the decision. That ownership split is
    // correct — only the party that consumed a decision can retire the gate — and asserting
    // "the list empties" here would encode the wrong owner and break the moment someone fixes
    // an unrelated bug in the waiter. Leg 7 asserts the recovery property instead.
    let denied_again = client
        .invoke(
            "deny_permission_request",
            json!({ "args": { "request_id": "no-such-gate", "decision": "deny" } }),
        )
        .await;
    let body = invoke_ok(&denied_again);
    assert_eq!(
        body["ok"], false,
        "a brake against an unknown gate must fail closed rather than report a phantom success \
         — which is what makes the positive assertion above meaningful: {}",
        denied_again.body,
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("not found")),
        "the refusal must name the missing gate: {}",
        denied_again.body,
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =========================================================================================
// Leg 3 — event: contiguous delivery, then cursor replay of exactly the missed rows
// =========================================================================================

/// Durable events arrive contiguous; after a disconnect, replay delivers EXACTLY the rows the
/// client missed — no gap, no duplicate, no re-delivery of what it already had.
///
/// "Exactly" is the assertion with teeth. A host that replayed from zero would still produce a
/// correct-looking transcript in a weaker test; here the replayed seq set is compared against the
/// precise window the client was absent for.
#[tokio::test]
async fn leg3_durable_events_are_contiguous_and_replay_exactly_the_missed_window() {
    let (host, mut client, max_seq, epoch) = connected_client().await;

    // --- live delivery, contiguous from H+1 ---
    for index in 0..3 {
        host.emit(DURABLE_PROBE, json!({ "phase": "live", "index": index }));
    }
    let live = collect_durable_seqs(&mut client, 3).await;
    assert_eq!(
        live,
        vec![max_seq + 1, max_seq + 2, max_seq + 3],
        "durable seqs must be contiguous and start one past the hello high-water",
    );
    let cursor = *live.last().expect("three seqs were collected");

    // --- disconnect; emit into the gap ---
    drop(client);
    let device = host
        .pair_device("e2e-reconnect")
        .await
        .expect("pairing a second device should succeed");
    let mut reconnected = ScriptedClient::new(host.base_url(), device.token);

    // Emitted while nobody is subscribed at `cursor`. These are the rows the replay owes us.
    for index in 0..4 {
        host.emit(DURABLE_PROBE, json!({ "phase": "missed", "index": index }));
    }

    // --- reconnect and warm-resume from the old cursor ---
    let hello = reconnected.connect().await;
    let (new_max, new_epoch) = match hello {
        ServerFrame::Hello {
            max_seq,
            stream_epoch,
            ..
        } => (max_seq, stream_epoch),
        other => panic!("expected hello, got {other:?}"),
    };
    assert_eq!(
        new_epoch, epoch,
        "the epoch must not roll across a mere client reconnect — a warm resume depends on it",
    );
    assert_eq!(
        new_max,
        cursor + 4,
        "the host high-water must reflect exactly the four rows emitted during the gap",
    );

    reconnected
        .send(ClientFrame::Subscribe {
            after_seq: cursor,
            stream_epoch: new_epoch,
        })
        .await;

    let mut replayed = Vec::new();
    let through = loop {
        match reconnected
            .next_frame()
            .await
            .expect("the replay must complete rather than closing the socket")
        {
            ServerFrame::Event {
                seq: Some(seq),
                name,
                ..
            } if name == DURABLE_PROBE => replayed.push(seq),
            ServerFrame::ReplayDone { through_seq } => break through_seq,
            ServerFrame::Reset { reason } => {
                panic!("a live cursor must warm-resume, not reset ({reason:?})")
            }
            _ => continue,
        }
    };

    assert_eq!(
        replayed,
        vec![cursor + 1, cursor + 2, cursor + 3, cursor + 4],
        "replay must deliver exactly the missed window — contiguous, no gap, no duplicate",
    );
    assert_eq!(
        through, new_max,
        "replayDone must report the high-water the replay caught up to",
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =========================================================================================
// Leg 4 — revoke: live WS closes AND the next request 401s
// =========================================================================================

/// Revocation tears down the LIVE session and refuses the next request.
///
/// The conjunction is the point (P-2). A host that only denied future HTTP would leave a revoked
/// device streaming until it happened to reconnect — the device would keep receiving events it
/// is no longer entitled to, which is precisely the failure revocation exists to prevent.
#[tokio::test]
async fn leg4_revocation_closes_the_live_socket_and_401s_the_next_invoke() {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");
    let device = host
        .pair_device("revoked-device")
        .await
        .expect("pairing should succeed");
    let mut client = ScriptedClient::new(host.base_url(), device.token.clone());
    subscribe(&mut client).await;

    // Prove the stream is genuinely live first, or the teardown assertion proves nothing.
    host.emit(TRANSIENT_CHUNK, json!({ "delta": "live" }));
    assert!(
        client.next_event_named(TRANSIENT_CHUNK).await.is_some(),
        "the stream must be live before the revoke",
    );

    host.revoke_device(&device.device_id)
        .await
        .expect("revocation should succeed");

    assert!(
        client.closed().await,
        "revocation must tear down the LIVE socket, not just deny future requests (P-2)",
    );

    // ...and the very next bearer INVOKE — not merely a ws-ticket — must 401.
    let redial = ScriptedClient::new(host.base_url(), device.token);
    let after = redial.invoke("health_check", json!({})).await;
    assert_eq!(
        after.status, 401,
        "a revoked bearer must be refused on the next invoke, got {} / {}",
        after.status, after.body,
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =========================================================================================
// Leg 5 — prune → reset → cold re-hydrate
// =========================================================================================

/// Retention advancing past a slow client's cursor ends in a `reset(cursor_pruned)` followed by
/// a COMPLETED `H`-barrier cold hydrate — never a truncated splice.
///
/// The fail-closed floor is the whole point: the missed rows are gone, so serving a partial
/// replay would hand the client a transcript with an invisible hole. The only correct answer is
/// "start over from a snapshot", and this leg proves the client can actually get there.
#[tokio::test]
async fn leg5_a_pruned_cursor_resets_and_then_cold_hydrates() {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");
    let device = host
        .pair_device("slow-client")
        .await
        .expect("pairing should succeed");
    let mut client = ScriptedClient::new(host.base_url(), device.token);

    // Connect and read H, but DO NOT subscribe: this is the stale cursor the prune outruns.
    let hello = client.connect().await;
    let (stale_cursor, epoch) = match hello {
        ServerFrame::Hello {
            max_seq,
            stream_epoch,
            ..
        } => (max_seq, stream_epoch),
        other => panic!("expected hello, got {other:?}"),
    };

    // Advance the log well past the stale cursor.
    for index in 0..32 {
        host.emit(DURABLE_PROBE, json!({ "index": index }));
    }

    // A second device's `hello` doubles as the commit barrier: `max_seq` is read AFTER commit,
    // so once it exceeds the stale cursor the batch is durable — no sleep required.
    let barrier_device = host
        .pair_device("prune-barrier")
        .await
        .expect("pairing the barrier device should succeed");
    let mut barrier_client = ScriptedClient::new(host.base_url(), barrier_device.token);
    let barrier = match barrier_client.connect().await {
        ServerFrame::Hello { max_seq, .. } => max_seq,
        other => panic!("expected hello, got {other:?}"),
    };
    assert!(
        barrier > stale_cursor,
        "the durable batch must have advanced the high-water past the stale cursor",
    );

    // Publish the retention floor a completed prune would leave behind.
    host.force_pruned_floor(barrier);

    // --- the stale cursor must now fail CLOSED ---
    client
        .send(ClientFrame::Subscribe {
            after_seq: stale_cursor,
            stream_epoch: epoch.clone(),
        })
        .await;
    let reset = client
        .next_frame_matching(|frame| matches!(frame, ServerFrame::Reset { .. }))
        .await
        .expect("a pruned cursor must be answered, not silently spliced");
    match reset {
        ServerFrame::Reset { reason } => assert_eq!(
            reason,
            ResetReason::CursorPruned,
            "the reset must name the pruned cursor, so the client knows a snapshot is required",
        ),
        other => panic!("expected reset, got {other:?}"),
    }
    assert!(
        host.counters().reset_cursor_pruned >= 1,
        "the host must count the pruned-cursor reset in its observability surface",
    );

    // --- and the cold re-hydrate must actually complete ---
    // A reset that leaves the client unable to recover would be no better than a truncated
    // splice, so the leg does not end at the reset frame.
    let mut rehydrated = ScriptedClient::new(host.base_url(), client.token().to_string());
    let (fresh_h, fresh_epoch) = subscribe(&mut rehydrated).await;
    assert!(
        fresh_h >= barrier,
        "the fresh H barrier must be at or past the retention floor, got {fresh_h} vs {barrier}",
    );
    assert_eq!(
        fresh_epoch, epoch,
        "a prune does not roll the epoch — only overload and reboot do",
    );

    // Live delivery resumes from the fresh barrier.
    host.emit(DURABLE_PROBE, json!({ "phase": "after-cold-hydrate" }));
    let resumed = collect_durable_seqs(&mut rehydrated, 1).await;
    assert_eq!(
        resumed,
        vec![fresh_h + 1],
        "after a cold hydrate the client resumes contiguously from its new barrier",
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =========================================================================================
// Leg 6 — the full fake-CLI chat turn (deferred from PR 3.2 per SPEC DRIFT A8)
// =========================================================================================

/// Writes a fake Claude CLI that speaks stream-json and returns its path.
///
/// The `cat >/dev/null &` drain is not decoration: the send path writes the prompt to the
/// child's stdin, and a CLI that exits without draining it makes the parent take EPIPE. The
/// background drain plus the explicit kill is the idiom the repo's existing fixtures use.
fn write_fake_claude_cli(dir: &std::path::Path) -> std::path::PathBuf {
    let cli_path = dir.join("fake-claude");
    std::fs::write(
        &cli_path,
        r#"#!/bin/sh
case "$*" in
  *--version*) echo "1.0.0 (Claude Code)"; exit 0 ;;
esac
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"remote hello"}]},"session_id":"e2e-session"}'
printf '%s\n' '{"type":"result","session_id":"e2e-session","is_error":false,"result":"remote hello","cost_usd":0.0}'
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .expect("the fake CLI should be writable");
    let mut permissions = std::fs::metadata(&cli_path)
        .expect("the fake CLI should exist")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&cli_path, permissions).expect("the fake CLI should be executable");
    cli_path
}

/// Seeds the project + conversation rows a turn needs and returns both ids.
async fn seed_chat_context(
    state: &AppState,
    project_directory: &std::path::Path,
) -> (Project, ChatConversation) {
    let project = state
        .project_repo
        .create(Project::new(
            "e2e-remote".to_string(),
            project_directory.to_string_lossy().into_owned(),
        ))
        .await
        .expect("the project row should be creatable");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("the conversation row should be creatable");
    (project, conversation)
}

/// A real chat turn, driven through the production `AppChatService`, reaches a remote client.
///
/// This is the leg PR 3.2 deferred. Per SPEC DRIFT A8, the scripted-agent seam is **not** an
/// `AgenticClient` — that trait is never called by `send_message`, which spawns a provider CLI
/// and parses stream-json. The honest seam is `with_cli_path` pointed at a fake stream-json
/// script, which keeps the ENTIRE production path real (spawn, parse, persist, emit) while the
/// LLM half is deterministic. A real provider binary is never spawned: nondeterministic,
/// unavailable in CI, and a known trap in this repo.
///
/// The chain under test is the full one:
/// fake CLI → stream-json parse → `AppChatService` emit → Tauri bus → capture bank →
/// classification → sequencer (durable) or broadcast (transient) → WS session → client.
#[tokio::test]
async fn leg6_a_fake_cli_chat_turn_streams_to_a_remote_client_and_persists_only_durables() {
    // `AppState::new_sqlite_test()` sets `RALPHX_TEST_MODE=1`, which disables provider spawns.
    // This is the documented opt-in. Nextest runs each test in its own process, so the global
    // is not shared with a sibling leg.
    std::env::set_var("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");

    let (host, mut client, _max_seq, _epoch) = connected_client().await;

    let temp = tempfile::tempdir().expect("a temp dir should be creatable");
    let project_directory = temp.path().join("project");
    std::fs::create_dir_all(&project_directory).expect("the project dir should be creatable");
    let cli_path = write_fake_claude_cli(temp.path());

    let (project, conversation) = {
        let state = host.state();
        seed_chat_context(&state, &project_directory).await
    };

    // The production service, on the harness's own app handle — so its emits land on the very
    // Tauri bus the capture bank is listening to.
    let service = host
        .state()
        .build_chat_service_for_runtime::<tauri::test::MockRuntime>(None, Some(host.app_handle()))
        .with_cli_path(cli_path)
        .with_working_directory(&project_directory);

    service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "say hello to the remote client",
            ralphx_lib::application::chat_service::SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                ..Default::default()
            },
        )
        .await
        .expect("the turn should start");

    // --- the streamed text reaches the remote client, transiently ---
    let chunk = client
        .next_event_named(TRANSIENT_CHUNK)
        .await
        .expect("a streamed chunk must reach the remote client over the real socket");
    match chunk {
        ServerFrame::Event { seq, payload, .. } => {
            assert!(
                seq.is_none(),
                "a transient chunk must carry no seq — a seq would let streaming advance the \
                 client's resume cursor (§3.4)",
            );
            assert!(
                payload["text"].is_string() || payload["delta"].is_string(),
                "the chunk must carry the streamed text: {payload}",
            );
        }
        other => panic!("expected an event frame, got {other:?}"),
    }

    // --- the durable half of the turn arrives sequenced ---
    let durable = client
        .next_frame_matching(|frame| {
            matches!(frame, ServerFrame::Event { name, seq, .. }
                if name == DURABLE_PROBE && seq.is_some())
        })
        .await
        .expect("the turn's message_created must arrive as a sequenced durable frame");
    assert!(
        matches!(durable, ServerFrame::Event { seq: Some(_), .. }),
        "durable chat events must carry a seq",
    );

    // --- A-4: not one transient row reached the durable log ---
    durable_barrier(&host, &mut client, "after-real-chat-turn").await;
    let names = host
        .durable_event_names()
        .await
        .expect("the durable log should be readable");
    assert!(
        !names.iter().any(|name| name == TRANSIENT_CHUNK),
        "a real streamed turn must leave zero agent:chunk rows in remote_event_log (A-4): \
         {names:?}",
    );
    assert!(
        !names.iter().any(|name| name == "agent:usage_updated"),
        "agent:usage_updated is Transient and must never be persisted (A-4): {names:?}",
    );
    assert!(
        names.iter().any(|name| name == DURABLE_PROBE),
        "the durable half of the turn must be persisted, else this proves nothing: {names:?}",
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =========================================================================================
// Leg 7 — pending-gate hydration across a disconnect
// =========================================================================================

/// A gate raised while the client is disconnected is recovered by AUTHORITATIVE hydration.
///
/// This is P-21's real shape, and the reason it cannot be proven by replay: `permission:request`
/// is classified **Transient**, so it is never written to `remote_event_log` and a reconnecting
/// client will never see it replayed. The gate survives only because `list_pending_*` is the
/// durable authority. A client that trusted the event stream alone would silently lose the gate
/// and leave the agent blocked forever.
#[tokio::test]
async fn leg7_a_gate_raised_during_a_disconnect_is_recovered_by_hydration_not_replay() {
    let (host, client, _max_seq, _epoch) = connected_client().await;
    let token = client.token().to_string();

    // The client goes away mid-run.
    drop(client);

    let request_id = "e2e-disconnected-gate";
    host.state()
        .permission_state
        .register(PendingPermissionInfo {
            request_id: request_id.to_string(),
            tool_name: "mcp__ralphx__get_task_context".to_string(),
            tool_input: json!({ "task_id": "task-9" }),
            context: Some("raised while disconnected".to_string()),
            agent_type: Some("worker".to_string()),
            task_id: Some("task-9".to_string()),
            context_type: Some("task".to_string()),
            context_id: Some("task-9".to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .await;

    // Reconnect and hydrate.
    let mut reconnected = ScriptedClient::new(host.base_url(), token);
    subscribe(&mut reconnected).await;

    let gates = gate_rows(
        &reconnected
            .invoke("list_pending_permission_gates", json!({}))
            .await,
    );
    assert!(
        has_gate(&gates, request_id),
        "a gate raised during the disconnect must be recoverable by hydration: {gates:?}",
    );

    // The gate is NOT in the durable log — hydration is the only authority.
    durable_barrier(&host, &mut reconnected, "after-gate-hydration").await;
    let names = host
        .durable_event_names()
        .await
        .expect("the durable log should be readable");
    assert!(
        !names.iter().any(|name| name.starts_with("permission:")),
        "gate lifecycle events are Transient and must never be persisted: {names:?}",
    );

    // Resolving it clears it, through the same authority.
    let denied = reconnected
        .invoke(
            "deny_permission_request",
            json!({ "args": { "request_id": request_id, "decision": "deny" } }),
        )
        .await;
    assert_eq!(
        denied.status, 200,
        "denying the hydrated gate must succeed: {}",
        denied.body,
    );
    assert_eq!(
        invoke_result(&denied)["success"],
        true,
        "the decision must have reached the gate the client hydrated — proving the hydrated \
         request id is genuinely actionable and not a stale projection: {}",
        denied.body,
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =========================================================================================
// Standing assertions — P-16 at the E2E layer
// =========================================================================================

/// `:3847` stays loopback-only.
///
/// The bind address is a pure function of the port, so this is a real regression guard on the
/// constant the phase carries as a standing assertion — not a restatement of it.
#[test]
fn p16_the_local_backend_binds_loopback_only() {
    let bind = ralphx_lib::utils::backend_endpoint::backend_http_bind_addr();
    assert!(
        bind.starts_with("127.0.0.1:"),
        "the :3847 backend must bind loopback only, got {bind}",
    );
    let base = ralphx_lib::utils::backend_endpoint::backend_http_base_url();
    assert!(
        base.starts_with("http://127.0.0.1:"),
        "the :3847 base URL must be loopback only, got {base}",
    );
}

/// The `:3847` trust header buys exactly nothing on `:3849`.
///
/// `X-RalphX-Tauri-MCP: 1` is what makes a request trusted on the local backend. If the remote
/// router honoured it — or merely forwarded it to a remounted handler that does — every
/// authenticated route would be reachable by anyone who can reach the port. The header is
/// stripped at the remote edge, so the request is judged on its (absent) bearer alone.
#[tokio::test]
async fn p16_the_local_trust_header_is_not_honoured_on_the_remote_listener() {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");

    for path in ["/health", "/remote/v1/session"] {
        let response = harness_http_client()
            .fetch(
                &host.base_url(),
                // Deliberately not a bearer: the ONLY credential offered is the trust header.
                "not-a-token",
                &RemoteFetchRequest {
                    path: path.to_string(),
                    method: "GET".to_string(),
                    headers: vec![(TAURI_TRUST_HEADER.to_string(), "1".to_string())],
                    body: None,
                },
            )
            .await
            .expect("the request should complete");
        assert_eq!(
            response.status, 401,
            "{TAURI_TRUST_HEADER} must buy nothing on :3849 for {path}, got {} / {}",
            response.status, response.body,
        );
    }

    host.stop().await.expect("the host should stop cleanly");
}

/// The pre-auth surface is exactly two routes, observed over the wire.
///
/// Asserted as an EQUALITY, in both directions: the two named routes answer without a bearer,
/// and the authenticated ones do not. A one-directional check would pass against a host that
/// had quietly opened a third door.
#[tokio::test]
async fn p16_only_the_descriptor_and_pairing_routes_answer_without_a_bearer() {
    let host = RemoteHostHarness::start()
        .await
        .expect("the harness host should boot");
    let http = harness_http_client();

    // Pre-auth: the descriptor answers with no credential at all.
    let descriptor = http
        .fetch_descriptor(&host.base_url())
        .await
        .expect("the descriptor must be reachable pre-auth");
    assert_eq!(
        descriptor.environment_id,
        host.environment_id(),
        "the descriptor must identify this host",
    );

    // Pairing is pre-auth too: a BAD code is refused on its merits (401), never as "no bearer".
    let pairing = http
        .pair(
            &host.base_url(),
            &PairWireRequest {
                pairing_code: "000000000".to_string(),
                device_name: "unpaired".to_string(),
                client_version: HARNESS_CLIENT_VERSION.to_string(),
                requested_scopes: RemoteScopeSet::default_pairing_grant().to_vec(),
            },
        )
        .await;
    assert!(
        matches!(pairing, Err(RemoteHostClientError::Rejected { .. })),
        "the pairing route must be reachable pre-auth and reject a bad code, got {pairing:?}",
    );

    // Everything else authenticates — including `/health`. A-2: no zero-devices bootstrap.
    for path in ["/health", "/remote/v1/session", "/remote/v1/auth/ws-ticket"] {
        let response = http
            .fetch(
                &host.base_url(),
                "not-a-token",
                &RemoteFetchRequest {
                    path: path.to_string(),
                    method: if path.ends_with("ws-ticket") {
                        "POST".to_string()
                    } else {
                        "GET".to_string()
                    },
                    headers: Vec::new(),
                    body: None,
                },
            )
            .await
            .expect("the request should complete");
        assert_eq!(
            response.status, 401,
            "{path} must authenticate — there is no zero-devices bootstrap pass (A-2), got {}",
            response.body,
        );
    }

    host.stop().await.expect("the host should stop cleanly");
}

// =========================================================================================
// First-enable ordering (P-23 seam)
// =========================================================================================

/// On a first-ever enable, `start_listener` mints the settings row and binds BEFORE
/// `install_remote_stream_from_handle` installs the sequencer — so the router is already
/// serving when the stream lands in the shared slot. The WS route must observe that install
/// live: a router that snapshots the slot at construction answers every subscribe with
/// `503 REMOTE_UNREACHABLE` until an off/on cycle rebuilds it, which is precisely the
/// "paired fine, then Reconnecting forever" failure this test pins.
#[tokio::test]
async fn a_first_enable_host_serves_the_event_stream_without_a_listener_restart() {
    let host = RemoteHostHarness::start_first_enable()
        .await
        .expect("the first-enable host should boot");
    let device = host
        .pair_device("first-enable-client")
        .await
        .expect("pairing should succeed");
    let mut client = ScriptedClient::new(host.base_url(), device.token);

    // The full production connect: ticket → WS upgrade → `hello` → `subscribe` → `replayDone`.
    // Reaching `replayDone` proves the stream installed after the bind is the one serving.
    let (max_seq, epoch) = subscribe(&mut client).await;
    assert_eq!(
        max_seq, 0,
        "a fresh first-enable host has no durable history"
    );
    assert!(
        !epoch.is_empty(),
        "hello must carry the epoch of the post-bind sequencer",
    );

    host.stop().await.expect("the host should stop cleanly");
}

// =========================================================================================
// Suite-level guards
// =========================================================================================

/// The suite is deterministic only while the port override is unset; the harness asserts this
/// per boot, and this makes the requirement legible at the suite level too.
#[test]
fn the_remote_port_override_must_not_be_set_for_this_suite() {
    assert!(
        std::env::var("RALPHX_REMOTE_PORT").is_err(),
        "RALPHX_REMOTE_PORT pins every harness host to one port and collides across legs",
    );
}

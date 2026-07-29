//! Two-instance chat-stream validation harness (PR 3.2).
//!
//! **This is a SHARED FIXTURE, not test-file-local plumbing.** Phase-3 open question 2 was
//! resolved in favour of "3.2 builds the minimal shared fixture 3.4 extends", so everything
//! here is designed for reuse by PR 3.4's `remote_e2e` suite: one `pub` API, no leg-specific
//! assumptions baked into the boot path.
//!
//! # Why the fixture lives inside the crate
//!
//! Every item the harness needs — `remote_router`, `RemoteRouterState`,
//! `start_listener_with_runtime`, `RemoteSequencer`, `RemoteStreamHandle`, `ws::WS_EVENTS_PATH`
//! — is `pub(crate)`. Only a module inside this crate can touch them. A gated child module of
//! `remote_server` can, and re-exposes exactly one `pub` surface, which is what lets 3.4 build
//! a `src-tauri/tests/remote_e2e.rs` integration binary on top with `--features test-utils`
//! instead of promoting ~15 internal items to `pub` one at a time.
//!
//! The gate is `feature = "test-utils"` rather than `cfg(test)` because
//! (a) production items must never carry `#[cfg(test)]`, and (b) the harness needs
//! `tauri::test::MockRuntime`, which IS the `test-utils` feature (`test-utils = ["tauri/test"]`).
//! This mirrors the established `src/testing/**` pattern.
//!
//! # What is real here
//!
//! The point of the harness is that the host is the production stack and the boundary under
//! test is a real socket. Nothing below is a mock router or an auth bypass:
//!
//! | Concern | Seam |
//! |---|---|
//! | Router | [`super::remote_router`] — real auth / CORS / rate-limit / trust-strip layers |
//! | Listener | real `TcpListener` on a free loopback port via [`super::start_listener_with_runtime`] |
//! | Pairing | the real `POST /remote/v1/auth/pair` over HTTP — no zero-devices bootstrap (A-2) |
//! | Capture | [`super::capture::RemoteEventCapture::install`] — the production fn, real Tauri listeners |
//! | Sequencer | [`super::sequencer::RemoteSequencer::start`] — the real one-actor sequencer |
//! | Client HTTP | [`HyperRemoteHostClient`] — the production client-side proxy |
//!
//! # The Wry `AppHandle` trap
//!
//! The host runs on `tauri::test::MockRuntime`, never Wry. Building a real Wry `AppHandle` off
//! the main thread panics on macOS (`On macOS, EventLoop must be created on the main thread!`)
//! — the documented cause of the 28-test `auth_tests` breakage this repo already fixed once.
//! `MockRuntime` has no such requirement, and `RemoteListenerRuntime::for_tests` +
//! `RemoteRouterState::new_with_invoke_dispatcher` are the two seams that let the listener boot
//! without a handle at all.

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use ralphx_remote_protocol::{ClientFrame, Scope, ServerFrame};
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::{Emitter, Manager};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::application::AppState;
use crate::domain::entities::{
    RemoteDeviceId, RemotePairingCode, RemotePairingCodeId, RemoteScopeSet,
};
use crate::domain::services::key_crypto::hash_key;
use crate::error::AppResult;
use crate::infrastructure::remote_host_client::{
    HyperRemoteHostClient, InvokeWireRequest, PairWireRequest, RemoteFetchRequest,
    RemoteHostClient, RemoteHttpResponse,
};
use crate::infrastructure::sqlite::{
    DbConnection, RemoteEventLogStore, SqliteRemoteEventLogRepository,
};
use crate::infrastructure::tailscale::{TailscaleCommandRunner, TailscaleServeError};
use crate::remote_server::auth::{
    expiry_timestamp, generate_pairing_code, now_timestamp, RemoteAuthContext,
    PAIRING_CODE_TTL_SECS,
};
use crate::remote_server::capture::{CaptureFeed, RemoteEventCapture};
use crate::remote_server::invoke::RemoteInvokeDispatcher;
use crate::remote_server::registry::{DispatchOutcome, RemoteInvokeError};
use crate::remote_server::retention::RetentionLeaseRegistry;
use crate::remote_server::sequencer::{EpochRollCause, RemoteSequencer, RemoteStreamHandle};
use crate::remote_server::settings::{
    RemoteExposureMode, RemoteHostSettingsStore, UnconfiguredTailnetProvider, REMOTE_PORT_ENV,
};
use crate::remote_server::ws::WS_EVENTS_PATH;
use crate::remote_server::{
    start_listener_with_runtime, stop_listener, RemoteListenerHandle, RemoteListenerRuntime,
    REMOTE_CAPTURE_QUEUE_CAPACITY,
};

/// Client version reported at pairing. The host audits it; omitting it logs "unknown client".
pub const HARNESS_CLIENT_VERSION: &str = "0.0.0-harness";

/// `POST /remote/v1/auth/ws-ticket` — the client mints a single-use ticket here because a
/// WebSocket cannot carry an `Authorization` header (§3.2).
const WS_TICKET_PATH: &str = "/remote/v1/auth/ws-ticket";

/// Row cap for durable-log reads. Far above anything a leg emits, so a truncated read can
/// never be mistaken for an absent row.
const DURABLE_READ_LIMIT: usize = 100_000;

// =====================================================================================
// Host side
// =====================================================================================

/// Serve exposure calls out to the `tailscale` CLI. The harness binds loopback directly, so
/// the runner is a no-op that reports success — it must never resolve a real binary
/// (`production-cli-resolution.md`: no bare production spawns, and a test must not depend on
/// whether the developer has Tailscale installed).
struct HarnessTailscale;

#[async_trait]
impl TailscaleCommandRunner for HarnessTailscale {
    async fn run_status(
        &self,
    ) -> Result<String, crate::remote_server::settings::TailnetProviderError> {
        Ok(String::new())
    }

    async fn run_serve_acquire(&self, _port: u16) -> Result<(), TailscaleServeError> {
        Ok(())
    }

    async fn run_serve_release(&self) -> Result<(), TailscaleServeError> {
        Ok(())
    }
}

/// Dispatches `/remote/v1/invoke` against the REAL `remote_commands!` allowlist.
///
/// This is the seam that keeps scope enforcement production-real: `registry::dispatch` runs
/// scope → authz → validate before the target fn is reached, so a harness leg that asserts
/// `REMOTE_FORBIDDEN` is exercising the shipped gate, not a re-implementation of it.
struct RegistryInvokeDispatcher {
    app: tauri::AppHandle<MockRuntime>,
}

#[async_trait]
impl RemoteInvokeDispatcher for RegistryInvokeDispatcher {
    async fn dispatch(
        &self,
        scopes: &[Scope],
        command: &str,
        args: &Value,
    ) -> Result<DispatchOutcome, RemoteInvokeError> {
        crate::remote_server::registry::dispatch(&self.app, scopes, command, args).await
    }
}

/// Reserves a free loopback port by binding `:0` and releasing it.
///
/// The reserve/rebind window is the harness's one genuine race: another process could take the
/// port between the drop and the listener's bind. It is unavoidable while the settings row
/// refuses port `0` (`CHECK (port BETWEEN 1 AND 65535)`), and it is the pattern the existing
/// `listener_tests` already rely on.
async fn reserve_loopback_port() -> u16 {
    let socket = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("a loopback port should be reservable");
    socket
        .local_addr()
        .expect("the reserved socket should report its address")
        .port()
}

/// A device paired through the real HTTP surface.
#[derive(Debug, Clone)]
pub struct PairedDevice {
    /// The long-term bearer (`rxd_live_…`).
    pub token: String,
    pub device_id: RemoteDeviceId,
}

/// A booted production host: real listener, real auth, real sequencer, real capture.
pub struct RemoteHostHarness {
    /// Held for its lifetime: dropping the `App` tears down the Tauri event loop the capture
    /// listeners are registered on.
    app: tauri::App<MockRuntime>,
    db: DbConnection,
    settings: RemoteHostSettingsStore,
    auth: RemoteAuthContext,
    listener: RemoteListenerHandle,
    stream: RemoteStreamHandle,
    address: SocketAddr,
    environment_id: String,
}

impl RemoteHostHarness {
    /// Boots the host exactly the way production does, on a free loopback port.
    ///
    /// The port is reserved by binding `127.0.0.1:0`, reading the assigned port, and dropping
    /// the socket before the real listener rebinds it — the same approach `listener_tests`
    /// uses. A true ephemeral bind would be preferable (no reserve/rebind window at all), but
    /// the settings row carries a `CHECK (port BETWEEN 1 AND 65535)` constraint, so port `0`
    /// cannot be persisted and the listener has no other way to be told "pick one".
    /// See the flake-risk note in the lane tracker.
    pub async fn start() -> AppResult<Self> {
        // `RALPHX_REMOTE_PORT` would override the configured port at bind time and pin every
        // harness host to one port, which breaks both the ephemeral bind and any parallel run.
        // Refusing loudly beats binding somewhere the caller did not ask for.
        assert!(
            std::env::var(REMOTE_PORT_ENV).is_err(),
            "{REMOTE_PORT_ENV} must not be set while the remote harness runs: it would override \
             the ephemeral port and collide across tests",
        );

        let state = AppState::new_sqlite_test();
        let db = state.db.clone();
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("the mock Tauri app should build");

        let settings = RemoteHostSettingsStore::from_db(db.clone());
        // Mints the row, which is what "host mode is configured" means — capture installation
        // and the listener both gate on its presence.
        let created = settings.get_or_create().await?;
        let environment_id = created.environment_id.clone();
        let port = i64::from(reserve_loopback_port().await);
        db.run(move |conn| {
            conn.execute(
                "UPDATE remote_host_settings SET port = ?1 WHERE id = 1",
                rusqlite::params![port],
            )
            .map(|_| ())
            .map_err(|error| crate::error::AppError::Database(error.to_string()))
        })
        .await?;

        // --- capture + durable sequencer: the body of `install_remote_stream_from_handle` ---
        let (feed, receivers) = CaptureFeed::channels(REMOTE_CAPTURE_QUEUE_CAPACITY);
        let log = Arc::new(SqliteRemoteEventLogRepository::from_db(db.clone()));
        let stream = RemoteSequencer::start(log, receivers, RetentionLeaseRegistry::new()).await?;
        // Installed only after the sequencer is live, so no captured event is dropped into a
        // channel nobody drains yet — the same ordering production uses.
        RemoteEventCapture::install(app.handle().clone(), feed);

        let listener = RemoteListenerHandle::new();
        assert!(
            listener.install_stream(stream.clone()),
            "the harness installs exactly one sequencer",
        );

        let auth = RemoteAuthContext::from_db(
            db.clone(),
            listener.sessions().clone(),
            RemoteExposureMode::Serve,
        );

        let runtime = RemoteListenerRuntime::for_tests(Arc::new(RegistryInvokeDispatcher {
            app: app.handle().clone(),
        }));
        let address = start_listener_with_runtime(
            runtime,
            &listener,
            &settings,
            &UnconfiguredTailnetProvider,
            &HarnessTailscale,
        )
        .await
        .expect("the harness listener should bind an ephemeral loopback port");

        Ok(Self {
            app,
            db,
            settings,
            auth,
            listener,
            stream,
            address,
            environment_id,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Base URL a client dials, e.g. `http://127.0.0.1:54321`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn environment_id(&self) -> &str {
        &self.environment_id
    }

    pub fn app_handle(&self) -> tauri::AppHandle<MockRuntime> {
        self.app.handle().clone()
    }

    pub fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    pub fn db(&self) -> DbConnection {
        self.db.clone()
    }

    pub(crate) fn stream(&self) -> &RemoteStreamHandle {
        &self.stream
    }

    /// The live durable high-water — the `H` a client would see at `hello`.
    ///
    /// `pub` because [`RemoteStreamHandle`] is `pub(crate)`: PR 3.4's `remote_e2e` integration
    /// binary is outside the crate and cannot name the handle at all, so the fixture exposes
    /// the two facts a leg actually needs rather than promoting the whole sequencer surface.
    pub fn max_seq(&self) -> u64 {
        self.stream.max_seq()
    }

    /// Publishes a retention floor, as a completed prune would.
    ///
    /// This is the deterministic seam for the prune → reset leg. Running the REAL pruner is not
    /// an option for a test: `retention::prune_once` hardcodes `RETAIN_ROWS = 50_000`, so
    /// reaching a nonzero retention ceiling against the real SQLite store would mean committing
    /// more than fifty thousand rows per run. `record_pruned_floor` publishes exactly the state
    /// a real prune leaves behind — the floor every `subscribe` is validated against
    /// (`ws::validate_subscribe`) — so the leg exercises the production fail-closed path with
    /// the arithmetic, not the outcome, stubbed.
    ///
    /// The floor only ever moves forward (`fetch_max`), so this can never mask a live lease.
    pub fn force_pruned_floor(&self, floor: u64) {
        self.stream.record_pruned_floor(floor);
    }

    /// The host's live observability counters (§5.5, R-11).
    ///
    /// The load legs assert against these because they are the only in-process proxy for
    /// "host memory stayed flat" available to a Rust test — there is no reliable per-test
    /// RSS or allocator assertion (phase doc PR 3.3, key point 3).
    pub fn counters(&self) -> crate::remote_server::counters::RemoteStreamCounterSnapshot {
        self.stream.counters().snapshot()
    }

    /// Emits through the production Tauri bus, which is what the capture bank listens on.
    ///
    /// This is the same call shape `AppChatService::emit_event` makes, so an event pushed here
    /// travels the identical path: `emit` → `listen_any` (inline) → `CaptureFeed::dispatch` →
    /// sequencer (durable) or broadcast (transient).
    pub fn emit(&self, name: &str, payload: Value) {
        self.app
            .emit(name, payload)
            .expect("the mock app should accept an emit");
    }

    /// Mints a pairing code the way `generate_remote_pairing_code` does.
    pub async fn mint_pairing_code(&self, scopes: RemoteScopeSet) -> AppResult<String> {
        let raw = generate_pairing_code();
        self.auth
            .pairing_codes
            .create(RemotePairingCode {
                id: RemotePairingCodeId::new(),
                code_hash: hash_key(&raw),
                scopes,
                created_at: now_timestamp(),
                expires_at: expiry_timestamp(chrono::Utc::now(), PAIRING_CODE_TTL_SECS),
                consumed_at: None,
            })
            .await?;
        Ok(raw)
    }

    /// Pairs a device over the real HTTP pairing route with the default grant
    /// ("viewer with brakes": `ui:read` + `ui:operate`, never `ui:agent`).
    pub async fn pair_device(&self, name: &str) -> AppResult<PairedDevice> {
        self.pair_device_with_scopes(name, RemoteScopeSet::default_pairing_grant())
            .await
    }

    /// Pairs a device holding exactly `scopes`, so the production authorization gate can be
    /// driven with a grant that is deliberately insufficient.
    pub async fn pair_device_with_scopes(
        &self,
        name: &str,
        scopes: RemoteScopeSet,
    ) -> AppResult<PairedDevice> {
        let requested_scopes = scopes.to_vec();
        let code = self.mint_pairing_code(scopes).await?;
        let client = harness_http_client();
        let response = client
            .pair(
                &self.base_url(),
                &PairWireRequest {
                    pairing_code: code,
                    device_name: name.to_string(),
                    client_version: HARNESS_CLIENT_VERSION.to_string(),
                    // The host intersects the request with the code's grant; asking for
                    // nothing would mint a device holding nothing.
                    requested_scopes,
                },
            )
            .await
            .expect("pairing over the real listener should succeed");
        Ok(PairedDevice {
            token: response.device_token,
            device_id: RemoteDeviceId::from_string(response.device_id),
        })
    }

    /// Flips the per-device agent-control grant through the production repository seam.
    ///
    /// `ui:agent` is deliberately NOT grantable at pairing time (the pairing grant validator
    /// refuses it), so this is the only honest way to reach an agent-control device.
    pub async fn grant_agent_control(&self, device_id: &RemoteDeviceId) -> AppResult<()> {
        let granted = RemoteScopeSet::default_pairing_grant().with(Scope::UiAgent);
        // `set_scopes` refuses revoked devices, so this can never widen a dead credential.
        self.auth.devices.set_scopes(device_id, &granted).await?;
        Ok(())
    }

    /// Revokes a device through the production repository seam.
    ///
    /// Idempotent host-side; the interesting behaviour is what it does to a LIVE session, which
    /// is what the revoke-mid-stream leg asserts.
    pub async fn revoke_device(&self, device_id: &RemoteDeviceId) -> AppResult<()> {
        self.auth
            .devices
            .revoke(device_id, &now_timestamp())
            .await?;
        Ok(())
    }

    /// Every name currently persisted in `remote_event_log`, in seq order, for the live epoch.
    ///
    /// This is the query behind the A-4 absence assertion: a transient frame that ever reached
    /// the durable log would show up here.
    pub async fn durable_event_names(&self) -> AppResult<Vec<String>> {
        Ok(self
            .durable_rows()
            .await?
            .into_iter()
            .map(|row| row.name)
            .collect())
    }

    /// Raw durable rows for the live epoch, seq-ascending.
    pub async fn durable_rows(
        &self,
    ) -> AppResult<Vec<crate::infrastructure::sqlite::RemoteEventRow>> {
        let log = SqliteRemoteEventLogRepository::from_db(self.db.clone());
        // `through_seq` is deliberately the largest value the store accepts (seqs are persisted
        // as SQLite INTEGER, so anything above i64::MAX is rejected as an overflow) rather than
        // the live `max_seq`: an absence assertion must catch a row even if the bug under test
        // wrote it WITHOUT advancing the high-water.
        log.read_range(
            self.stream.epoch().as_str(),
            0,
            i64::MAX as u64,
            DURABLE_READ_LIMIT,
        )
        .await
    }

    /// Rolls the live in-memory epoch, which must make every connected client `reset`.
    pub fn roll_epoch(&self, cause: EpochRollCause) {
        self.stream.request_epoch_roll(cause);
    }

    /// Graceful teardown: persists the disabled flag, tears down live sessions, releases the port.
    pub async fn stop(self) -> AppResult<()> {
        stop_listener(&self.listener, &self.settings, &HarnessTailscale)
            .await
            .map_err(|error| crate::error::AppError::Database(error.to_string()))?;
        Ok(())
    }
}

// =====================================================================================
// Client side
// =====================================================================================

/// The production client-side HTTP proxy.
///
/// Deliberately the shipped `HyperRemoteHostClient` rather than a hand-rolled request builder:
/// it is the code a real client environment runs, so a serialization or header regression on
/// the client half fails the harness instead of hiding behind bespoke test plumbing.
pub fn harness_http_client() -> HyperRemoteHostClient {
    HyperRemoteHostClient::new().expect("the production hyper client should build")
}

/// A scripted client speaking the real protocol over a real socket.
pub struct ScriptedClient {
    base_url: String,
    token: String,
    http: HyperRemoteHostClient,
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

impl ScriptedClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: token.into(),
            http: harness_http_client(),
            socket: None,
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// `POST /remote/v1/invoke` with a client-minted `requestId`.
    ///
    /// The id is minted per call and passed through untouched, which is what the host binds for
    /// mutation dedup — a caller replaying an outcome must reuse the SAME id, so that is an
    /// explicit argument on [`Self::invoke_with_request_id`] rather than a hidden re-mint.
    pub async fn invoke(&self, cmd: &str, args: Value) -> RemoteHttpResponse {
        self.invoke_with_request_id(&uuid::Uuid::new_v4().to_string(), cmd, args)
            .await
    }

    pub async fn invoke_with_request_id(
        &self,
        request_id: &str,
        cmd: &str,
        args: Value,
    ) -> RemoteHttpResponse {
        self.http
            .invoke(
                &self.base_url,
                &self.token,
                &InvokeWireRequest {
                    request_id: request_id.to_string(),
                    cmd: cmd.to_string(),
                    args,
                },
            )
            .await
            .expect("an invoke against the harness listener should complete")
    }

    /// Mints a single-use WS ticket over the real bearer-authenticated route.
    pub async fn mint_ws_ticket(&self) -> RemoteHttpResponse {
        self.http
            .fetch(
                &self.base_url,
                &self.token,
                &RemoteFetchRequest {
                    path: WS_TICKET_PATH.to_string(),
                    method: "POST".to_string(),
                    headers: Vec::new(),
                    body: None,
                },
            )
            .await
            .expect("a ws-ticket request should complete")
    }

    /// Full connect: ticket → WS upgrade → `hello`.
    ///
    /// Returns the `hello` frame, which carries the `streamEpoch` and `maxSeq` the caller needs
    /// for the `H`-barrier and for `subscribe`.
    pub async fn connect(&mut self) -> ServerFrame {
        let ticket_response = self.mint_ws_ticket().await;
        assert_eq!(
            ticket_response.status, 200,
            "ws-ticket should be issued to a live bearer: {}",
            ticket_response.body
        );
        let ticket = serde_json::from_str::<Value>(&ticket_response.body)
            .expect("the ws-ticket response should be JSON")["ticket"]
            .as_str()
            .expect("a ticket is returned")
            .to_string();

        let url = format!(
            "ws://{}{}?ticket={}",
            self.base_url
                .trim_start_matches("http://")
                .trim_start_matches("https://"),
            WS_EVENTS_PATH,
            ticket
        );
        let (socket, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("the WS upgrade should succeed for a valid ticket");
        self.socket = Some(socket);
        self.next_frame().await.expect("hello should arrive first")
    }

    /// Sends a client frame.
    pub async fn send(&mut self, frame: ClientFrame) {
        use futures::SinkExt;
        let text = serde_json::to_string(&frame).expect("a client frame should serialize");
        self.socket
            .as_mut()
            .expect("the client must be connected")
            .send(Message::Text(text))
            .await
            .expect("sending a client frame should succeed");
    }

    /// Next server frame, or `None` once the socket closes.
    ///
    /// Event-driven: this awaits the socket rather than polling on a timer, which is what keeps
    /// the legs deterministic (no sleeps, no wall-clock races).
    pub async fn next_frame(&mut self) -> Option<ServerFrame> {
        use futures::StreamExt;
        let socket = self.socket.as_mut().expect("the client must be connected");
        while let Some(message) = socket.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    return Some(serde_json::from_str(&text).unwrap_or_else(|error| {
                        panic!("unparseable server frame {text}: {error}")
                    }));
                }
                // Ping/pong are transport noise; the protocol's own heartbeat is a Text frame.
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => continue,
                Ok(Message::Close(_)) => return None,
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
        None
    }

    /// Reads frames until one matches `predicate`, returning it.
    ///
    /// Returns `None` if the socket closes first. Bounded by the socket, not by a timer.
    pub async fn next_frame_matching(
        &mut self,
        mut predicate: impl FnMut(&ServerFrame) -> bool,
    ) -> Option<ServerFrame> {
        while let Some(frame) = self.next_frame().await {
            if predicate(&frame) {
                return Some(frame);
            }
        }
        None
    }

    /// Reads event frames until one named `name` arrives.
    pub async fn next_event_named(&mut self, name: &str) -> Option<ServerFrame> {
        self.next_frame_matching(
            |frame| matches!(frame, ServerFrame::Event { name: n, .. } if n == name),
        )
        .await
    }

    /// Whether the socket has closed (used by the revoke leg).
    pub async fn closed(&mut self) -> bool {
        self.next_frame_matching(|_| false).await.is_none()
    }
}

/// Convenience: the JSON body of a successful invoke.
pub fn invoke_ok(response: &RemoteHttpResponse) -> Value {
    assert_eq!(
        response.status, 200,
        "invoke should succeed: {}",
        response.body
    );
    serde_json::from_str(&response.body).expect("an invoke response should be JSON")
}

/// Convenience: the machine-readable error code of a refused invoke.
pub fn invoke_error_code(response: &RemoteHttpResponse) -> String {
    serde_json::from_str::<Value>(&response.body)
        .ok()
        .and_then(|body| body["code"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("<unparseable body: {}>", response.body))
}

/// A JSON payload for a synthetic emit.
pub fn payload(value: Value) -> Value {
    json!(value)
}

// RemoteWsClient — the client→host WebSocket transport seam (PR 2.3, §3.2).
//
// Deliberately thin: it dials, decodes `ServerFrame`s, and encodes `ClientFrame`s.
// It carries NO retry logic (A-5: the TS supervisor is the sole retry owner) and NO
// protocol state — hello handling, heartbeat acking, and session bookkeeping live in
// `application::remote_event_relay`, where they are testable against the mock below.
//
// Trait-based (like `RemoteHostClient`) so the relay runs against a scripted socket
// in unit tests instead of a real TCP upgrade.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use ralphx_remote_protocol::{ClientFrame, ServerFrame};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::infrastructure::remote_host_client::install_rustls_crypto_provider;

#[cfg(test)]
#[path = "remote_ws_client_tests.rs"]
mod tests;

/// The host's event stream endpoint (mirrors `remote_server::ws::WS_EVENTS_PATH`).
pub const REMOTE_EVENTS_PATH: &str = "/remote/v1/events";

/// Typed failures of the outbound WS transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteWsError {
    /// The dial provably did nothing (refused connection, bad URL, timeout before
    /// the upgrade completed).
    Unreachable(String),
    /// The handshake reached the host and the host answered with an HTTP refusal.
    /// 401/403 are the supervisor's `blocked` entries — they must stay
    /// distinguishable from a dead host.
    Rejected { status: u16, message: String },
    /// The socket spoke, but not the protocol: an undecodable text frame, a binary
    /// frame, or a frame the current state cannot accept.
    Protocol(String),
    /// The socket ended or refused a send.
    Closed(String),
}

impl RemoteWsError {
    /// Compact human-readable form for `remote:stream_closed` payloads and logs.
    pub fn short_reason(&self) -> String {
        match self {
            Self::Unreachable(message) => format!("unreachable: {message}"),
            Self::Rejected { status, message } => format!("rejected ({status}): {message}"),
            Self::Protocol(message) => format!("protocol error: {message}"),
            Self::Closed(message) => format!("closed: {message}"),
        }
    }
}

/// Builds the `ws(s)://…/remote/v1/events?ticket=…` dial URL from a paired
/// environment's base URL and a freshly minted single-use ticket.
///
/// The ticket is shape-validated BEFORE interpolation: it becomes part of a URL, so
/// an unvalidated value is a request-forgery seam (`?`/`&`/`#` would splice query
/// keys or truncate the path). Host-minted tickets are URL-safe base64, so the
/// accepted alphabet is exactly `[A-Za-z0-9_-]`.
pub fn ws_events_url(base_url: &str, ticket: &str) -> Result<String, RemoteWsError> {
    if ticket.is_empty()
        || !ticket
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(RemoteWsError::Unreachable(
            "ws ticket has an invalid shape and was not sent".to_string(),
        ));
    }
    let trimmed = base_url.trim().trim_end_matches('/');
    let ws_base = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        return Err(RemoteWsError::Unreachable(format!(
            "base URL has no WebSocket form (not http/https): {base_url}"
        )));
    };
    if ws_base.ends_with("://") {
        return Err(RemoteWsError::Unreachable(format!(
            "base URL has no host: {base_url}"
        )));
    }
    Ok(format!("{ws_base}{REMOTE_EVENTS_PATH}?ticket={ticket}"))
}

/// Decodes one wire text frame. A decode failure is a typed `Protocol` error, never
/// a silent skip — an undecodable frame means the client and host disagree about the
/// protocol, and skipping it would splice an invisible hole into the stream.
fn decode_server_frame(text: &str) -> Result<ServerFrame, RemoteWsError> {
    serde_json::from_str(text)
        .map_err(|error| RemoteWsError::Protocol(format!("unrecognized server frame: {error}")))
}

/// Dials the host's event stream endpoint.
#[async_trait]
pub trait RemoteWsClient: Send + Sync {
    async fn connect(&self, ws_url: &str) -> Result<Box<dyn RemoteWsConnection>, RemoteWsError>;
}

/// One live socket. Implementations MUST be cancel-safe in [`Self::recv`]: the relay
/// loop `select!`s on it and drops the future whenever another branch wins.
#[async_trait]
pub trait RemoteWsConnection: Send {
    /// `None` = the socket ended (peer close or EOF). `Some(Err(_))` = an unusable
    /// frame or a transport error; the relay tears the session down.
    async fn recv(&mut self) -> Option<Result<ServerFrame, RemoteWsError>>;
    async fn send(&mut self, frame: ClientFrame) -> Result<(), RemoteWsError>;
    async fn close(&mut self);
}

// ============================================================================
// Production implementation using tokio-tungstenite
// ============================================================================

pub struct TungsteniteRemoteWsClient {
    /// Budget for the whole dial (TCP + TLS + upgrade). Matches the HTTP client's
    /// request budget: a host that cannot complete an upgrade in this window is not
    /// serving a stream this session should wait on.
    connect_timeout: Duration,
}

impl TungsteniteRemoteWsClient {
    pub fn new() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
        }
    }
}

impl Default for TungsteniteRemoteWsClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps a tungstenite handshake failure into the transport taxonomy. Only an HTTP
/// answer becomes `Rejected` — everything else provably never reached an admitting
/// host and stays `Unreachable`.
fn handshake_error(error: tokio_tungstenite::tungstenite::Error) -> RemoteWsError {
    match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            let status = response.status().as_u16();
            let message: String = response
                .body()
                .as_deref()
                .map(|body| String::from_utf8_lossy(body).chars().take(300).collect())
                .unwrap_or_default();
            RemoteWsError::Rejected { status, message }
        }
        other => RemoteWsError::Unreachable(other.to_string()),
    }
}

#[async_trait]
impl RemoteWsClient for TungsteniteRemoteWsClient {
    async fn connect(&self, ws_url: &str) -> Result<Box<dyn RemoteWsConnection>, RemoteWsError> {
        // Same provider the HTTP client installs; whichever seam dials first wins the
        // `Once`, and the rustls default is set before any TLS handshake starts.
        install_rustls_crypto_provider();
        let (stream, _response) = tokio::time::timeout(
            self.connect_timeout,
            tokio_tungstenite::connect_async(ws_url),
        )
        .await
        .map_err(|_| {
            RemoteWsError::Unreachable(format!(
                "no WebSocket upgrade after {}s",
                self.connect_timeout.as_secs()
            ))
        })?
        .map_err(handshake_error)?;
        Ok(Box::new(TungsteniteRemoteWsConnection { stream }))
    }
}

struct TungsteniteRemoteWsConnection {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

#[async_trait]
impl RemoteWsConnection for TungsteniteRemoteWsConnection {
    async fn recv(&mut self) -> Option<Result<ServerFrame, RemoteWsError>> {
        loop {
            // `StreamExt::next` is cancel-safe, so dropping this future in `select!`
            // loses nothing.
            match self.stream.next().await? {
                Ok(Message::Text(text)) => return Some(decode_server_frame(&text)),
                // Ping/Pong are answered by tungstenite itself; binary frames are not
                // part of the v1 protocol (§3.2).
                Ok(Message::Binary(_)) => {
                    return Some(Err(RemoteWsError::Protocol(
                        "binary frames are not part of the remote protocol".to_string(),
                    )))
                }
                Ok(Message::Close(_)) => return None,
                Ok(_) => continue,
                Err(error) => return Some(Err(RemoteWsError::Closed(error.to_string()))),
            }
        }
    }

    async fn send(&mut self, frame: ClientFrame) -> Result<(), RemoteWsError> {
        let text = serde_json::to_string(&frame).map_err(|error| {
            RemoteWsError::Protocol(format!("client frame is not serializable: {error}"))
        })?;
        self.stream
            .send(Message::Text(text))
            .await
            .map_err(|error| RemoteWsError::Closed(error.to_string()))
    }

    async fn close(&mut self) {
        let _ = self.stream.close(None).await;
    }
}

// ============================================================================
// Test mock — the scripted socket the relay/service tests run against
// ============================================================================

/// Scripted WS endpoint (mirrors `MockRemoteHostClient`): each `connect` pops the
/// next scripted connection or error; every dialed URL is recorded for assertion.
pub struct MockRemoteWsClient {
    scripts: StdMutex<VecDeque<Result<MockRemoteWsConnection, RemoteWsError>>>,
    dialed: StdMutex<Vec<String>>,
}

impl MockRemoteWsClient {
    pub fn new() -> Self {
        Self {
            scripts: StdMutex::new(VecDeque::new()),
            dialed: StdMutex::new(Vec::new()),
        }
    }

    /// Scripts the next dial to succeed with `connection`.
    pub fn script_connection(&self, connection: MockRemoteWsConnection) {
        self.scripts
            .lock()
            .expect("mock ws scripts poisoned")
            .push_back(Ok(connection));
    }

    /// Scripts the next dial to fail.
    pub fn script_error(&self, error: RemoteWsError) {
        self.scripts
            .lock()
            .expect("mock ws scripts poisoned")
            .push_back(Err(error));
    }

    pub fn dialed_urls(&self) -> Vec<String> {
        self.dialed
            .lock()
            .expect("mock ws dial log poisoned")
            .clone()
    }
}

impl Default for MockRemoteWsClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RemoteWsClient for MockRemoteWsClient {
    async fn connect(&self, ws_url: &str) -> Result<Box<dyn RemoteWsConnection>, RemoteWsError> {
        self.dialed
            .lock()
            .expect("mock ws dial log poisoned")
            .push(ws_url.to_string());
        match self
            .scripts
            .lock()
            .expect("mock ws scripts poisoned")
            .pop_front()
        {
            Some(Ok(connection)) => Ok(Box::new(connection)),
            Some(Err(error)) => Err(error),
            None => Err(RemoteWsError::Unreachable(
                "no scripted mock connection".to_string(),
            )),
        }
    }
}

/// One scripted socket. Inbound frames are injected through the paired
/// [`MockRemoteWsHandle`]; outbound frames and the close call are observable there.
pub struct MockRemoteWsConnection {
    inbound: mpsc::UnboundedReceiver<Result<ServerFrame, RemoteWsError>>,
    outbound: mpsc::UnboundedSender<ClientFrame>,
    closed: Arc<AtomicBool>,
}

/// The test's end of a [`MockRemoteWsConnection`]. Dropping `inbound` ends the
/// scripted socket (recv → `None`), which is how tests simulate a peer close.
pub struct MockRemoteWsHandle {
    pub inbound: mpsc::UnboundedSender<Result<ServerFrame, RemoteWsError>>,
    pub outbound: mpsc::UnboundedReceiver<ClientFrame>,
    closed: Arc<AtomicBool>,
}

impl MockRemoteWsHandle {
    pub fn was_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

impl MockRemoteWsConnection {
    pub fn scripted() -> (Self, MockRemoteWsHandle) {
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let closed = Arc::new(AtomicBool::new(false));
        (
            Self {
                inbound: inbound_rx,
                outbound: outbound_tx,
                closed: Arc::clone(&closed),
            },
            MockRemoteWsHandle {
                inbound: inbound_tx,
                outbound: outbound_rx,
                closed,
            },
        )
    }
}

#[async_trait]
impl RemoteWsConnection for MockRemoteWsConnection {
    async fn recv(&mut self) -> Option<Result<ServerFrame, RemoteWsError>> {
        self.inbound.recv().await
    }

    async fn send(&mut self, frame: ClientFrame) -> Result<(), RemoteWsError> {
        self.outbound
            .send(frame)
            .map_err(|_| RemoteWsError::Closed("mock outbound receiver dropped".to_string()))
    }

    async fn close(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        self.inbound.close();
    }
}

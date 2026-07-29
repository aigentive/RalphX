// RemoteHostClient — trait abstraction for the client→host pairing/auth wire (§4.2).
//
// Trait-based (like WebhookHttpClient) so the pairing/reconciler logic can run against
// a scripted mock host in unit tests. Production uses hyper 1.x (no reqwest); it must
// speak BOTH https (Serve / MagicDNS certs) and plain http (direct 100.x endpoints).
//
// The device token flows through this seam only between the host response and the
// Keychain write — it is never returned to the webview (P-18).

use std::sync::Mutex;

use async_trait::async_trait;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use ralphx_remote_protocol::{EnvironmentDescriptor, Scope};
use serde::{Deserialize, Serialize};
use tokio::time::Duration;
use tokio_util::bytes::Bytes;

// NOTE (C-11 placement): `PairWire*` duplicates the host's `PairRequest`/`PairResponse`
// (`remote_server/auth_endpoints.rs`) instead of living in `ralphx-remote-protocol` with
// `EnvironmentDescriptor`/`Scope`. Until PR 1.4/2.2 move the pairing/invoke wire types into
// the protocol crate, the parity test in `remote_server/auth_tests.rs` is what ties the two
// definitions together — a host-side rename must fail that test, not production pairing.

/// Pre-auth descriptor route (mirrors `remote_server::DESCRIPTOR_PATH`).
pub const REMOTE_DESCRIPTOR_PATH: &str = "/.well-known/ralphx/environment";
/// Pairing exchange route (§4.2).
pub const REMOTE_PAIR_PATH: &str = "/remote/v1/auth/pair";
/// Bearer-authenticated session introspection route; a 200 proves the token is live.
pub const REMOTE_SESSION_PATH: &str = "/remote/v1/session";
/// Self-revocation route used by the staged remove machine (best-effort).
pub const REMOTE_REVOKE_PATH: &str = "/remote/v1/auth/revoke";
/// Single-use WS ticket mint (mirrors `remote_server::WS_TICKET_PATH`, §3.2).
pub const REMOTE_WS_TICKET_PATH: &str = "/remote/v1/auth/ws-ticket";

/// Wire request for `POST /remote/v1/auth/pair` (§4.2, C-11: camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairWireRequest {
    pub pairing_code: String,
    pub device_name: String,
    /// §3.1 defines it and the host audits it; omitting it logs every real pairing as
    /// "unknown client".
    pub client_version: String,
    pub requested_scopes: Vec<Scope>,
}

/// Wire response for `POST /remote/v1/auth/pair` (§4.2, C-11: camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairWireResponse {
    /// The long-term device token (`rxd_live_…`). Keychain-bound; never reaches JS.
    pub device_token: String,
    pub device_id: String,
    pub scopes: Vec<Scope>,
    pub environment_id: String,
    /// The version the host reports on the AUTHENTICATED exchange. `Option` because a host
    /// older than this field omits it; when present it is preferred over the pre-auth
    /// descriptor's copy for the stored row.
    #[serde(default)]
    pub protocol_version: Option<u32>,
}

/// Wire request for `POST /remote/v1/invoke` (§3.1, C-11: camelCase).
///
/// `requestId` is the client-minted UUID the host binds to a `cmd`+args hash for
/// mutation dedup (§3.3). It is generated per call in `network-invoke.ts` and passed
/// through untouched — the proxy must never re-mint it, or a client retrying a lost
/// response would lose its dedup identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvokeWireRequest {
    pub request_id: String,
    pub cmd: String,
    pub args: serde_json::Value,
}

/// One authenticated request against a remounted `/api/…` route (§3.5).
///
/// `path`, `method`, and `headers` are already validated by the application layer;
/// this struct is the post-validation shape, not the raw JS input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFetchRequest {
    /// Absolute, host-relative path beginning with `/`.
    pub path: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// A host answer, uninterpreted: status, response headers, and the raw body text.
///
/// The client deliberately does NOT classify statuses — what a 403 or a 409 means is
/// protocol policy and lives in the application layer, where the error taxonomy is.
///
/// `headers` is likewise UNFILTERED: which of the host's headers may cross into the
/// webview is protocol policy, so the response-side allowlist lives beside the
/// request-side one in `remote_environment_service`. Names are lowercased, order is
/// preserved, and duplicates are kept — real HTTP allows repeats and collapsing them
/// here would silently rewrite the host's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Typed failures from the client→host wire.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RemoteHostClientError {
    #[error("host unreachable: {0}")]
    Unreachable(String),
    /// The request was sent but no answer arrived in time. Distinct from
    /// `Unreachable` on purpose: a timed-out mutation may already have been applied
    /// host-side, so it is an UNKNOWN outcome (§3.3) and must never be retried
    /// blindly, whereas a refused connection provably did nothing.
    #[error("host did not answer in time: {0}")]
    Timeout(String),
    /// The host answered with a non-success status (bad pairing code, revoked token, …).
    #[error("host rejected the request ({status}): {message}")]
    Rejected { status: u16, message: String },
    #[error("host returned an invalid response: {0}")]
    InvalidResponse(String),
}

/// Client side of the host pairing/auth surface.
#[async_trait]
pub trait RemoteHostClient: Send + Sync {
    /// `GET /.well-known/ralphx/environment` — pre-auth identity + version negotiation.
    async fn fetch_descriptor(
        &self,
        base_url: &str,
    ) -> Result<EnvironmentDescriptor, RemoteHostClientError>;

    /// `POST /remote/v1/auth/pair` — exchanges a single-use pairing code for a device token.
    async fn pair(
        &self,
        base_url: &str,
        request: &PairWireRequest,
    ) -> Result<PairWireResponse, RemoteHostClientError>;

    /// Proves whether `token` is still a live bearer on the host.
    ///
    /// `Ok(true)` = live, `Ok(false)` = the host explicitly refused it (401/403);
    /// transport failures stay errors so callers can fail closed instead of
    /// mistaking an unreachable host for a revoked token.
    async fn validate_token(
        &self,
        base_url: &str,
        token: &str,
    ) -> Result<bool, RemoteHostClientError>;

    /// Best-effort self-revocation of `token` on the host (staged remove, P-27).
    async fn revoke_token(&self, base_url: &str, token: &str) -> Result<(), RemoteHostClientError>;

    /// `POST /remote/v1/invoke` — one bearer-authenticated command dispatch (§3.1).
    ///
    /// Returns the raw status/body; classification is the caller's job. No retries at
    /// this layer (A-5) — a resend would change the outcome of a dedup-guarded
    /// mutation from "unknown" to "possibly applied twice".
    async fn invoke(
        &self,
        base_url: &str,
        token: &str,
        request: &InvokeWireRequest,
    ) -> Result<RemoteHttpResponse, RemoteHostClientError>;

    /// One bearer-authenticated request against a remounted `/api/…` route (§3.5).
    async fn fetch(
        &self,
        base_url: &str,
        token: &str,
        request: &RemoteFetchRequest,
    ) -> Result<RemoteHttpResponse, RemoteHostClientError>;
}

/// `POST /remote/v1/invoke` (mirrors `remote_server::mod` route constants).
pub const REMOTE_INVOKE_PATH: &str = "/remote/v1/invoke";

// ============================================================================
// Production implementation using hyper 1.x
// ============================================================================

pub struct HyperRemoteHostClient {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    request_timeout: Duration,
    /// Separate, longer budget for command dispatch. §6.3 pins the client-visible
    /// invoke timeout at 30 s; the 15 s pairing/health budget would turn a slow but
    /// healthy command into a spurious unknown outcome.
    dispatch_timeout: Duration,
}

/// Installs the aws-lc-rs rustls provider exactly once. `pub(crate)` because the
/// outbound WS client (`remote_ws_client`) must set the same default before dialing.
pub(crate) fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: std::sync::Once = std::sync::Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

impl HyperRemoteHostClient {
    pub fn new() -> Result<Self, String> {
        install_rustls_crypto_provider();
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(Duration::from_secs(10)));
        connector.enforce_http(false);
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|error| format!("native root certificates unavailable: {error}"))?
            .https_or_http()
            .enable_http1()
            .wrap_connector(connector);
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(https),
            request_timeout: Duration::from_secs(15),
            dispatch_timeout: Duration::from_secs(30),
        })
    }

    async fn request_json(
        &self,
        method: Method,
        url: &str,
        bearer: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<(StatusCode, Vec<u8>), RemoteHostClientError> {
        // The pairing/auth surfaces are protocol endpoints this client parses itself,
        // so their response headers are noise; only the proxied `/api/…` surface
        // forwards them.
        let (status, _headers, body) = self
            .send(
                method,
                url,
                bearer,
                &[("Content-Type".to_string(), "application/json".to_string())],
                body,
                self.request_timeout,
            )
            .await?;
        Ok((status, body))
    }

    async fn send(
        &self,
        method: Method,
        url: &str,
        bearer: Option<&str>,
        headers: &[(String, String)],
        body: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<(StatusCode, Vec<(String, String)>, Vec<u8>), RemoteHostClientError> {
        let uri: hyper::Uri = url
            .parse()
            .map_err(|error| RemoteHostClientError::Unreachable(format!("invalid URL: {error}")))?;
        let mut builder = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        // Set last so a caller-supplied header can never displace the bearer.
        if let Some(bearer) = bearer {
            builder = builder.header("Authorization", format!("Bearer {bearer}"));
        }
        let request = builder
            .body(Full::new(Bytes::from(body.unwrap_or_default())))
            .map_err(|error| {
                RemoteHostClientError::Unreachable(format!("build request: {error}"))
            })?;
        let response = tokio::time::timeout(timeout, self.client.request(request))
            .await
            .map_err(|_| {
                // Timeout, not Unreachable: the request WAS sent, so the outcome is
                // unknown rather than provably nothing (§3.3).
                RemoteHostClientError::Timeout(format!("no answer after {}s", timeout.as_secs()))
            })?
            .map_err(|error| RemoteHostClientError::Unreachable(error.to_string()))?;
        let status = response.status();
        // Lowercased, order-preserving, duplicates kept: `HeaderMap`'s iterator yields
        // one entry per value, which is the shape the response allowlist filters.
        let response_headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_ascii_lowercase(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|error| RemoteHostClientError::Unreachable(error.to_string()))?
            .to_bytes()
            .to_vec();
        Ok((status, response_headers, body))
    }
}

/// Decodes a host body as UTF-8 text, tolerating invalid sequences.
///
/// A lossy decode is deliberate: a malformed body must still reach the caller with
/// its STATUS intact, because the status is what the error taxonomy keys on. Failing
/// the whole request on a decode error would convert a typed `REMOTE_FORBIDDEN` into
/// an untyped transport failure.
fn body_text(body: Vec<u8>) -> String {
    String::from_utf8_lossy(&body).into_owned()
}

fn join_url(base_url: &str, path: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), path)
}

/// Whether `status` proves the host no longer holds the token.
///
/// A host that answered at all — 2xx, or 401/403 for a bearer it refuses — has nothing left
/// to revoke. **404 is deliberately excluded**: the remote router answers it from its
/// fallback when a route is absent, so counting it as success would report every revoke
/// against a host without the route as done while its device row stayed live — precisely the
/// orphaned valid-but-unreferenced bearer §6.1/P-27 exists to prevent. Callers treat the
/// error as the best-effort residual it is.
fn revoke_completed(status: StatusCode) -> bool {
    status.is_success() || status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
}

fn rejected(status: StatusCode, body: &[u8]) -> RemoteHostClientError {
    RemoteHostClientError::Rejected {
        status: status.as_u16(),
        message: String::from_utf8_lossy(body).chars().take(300).collect(),
    }
}

#[async_trait]
impl RemoteHostClient for HyperRemoteHostClient {
    async fn fetch_descriptor(
        &self,
        base_url: &str,
    ) -> Result<EnvironmentDescriptor, RemoteHostClientError> {
        let url = join_url(base_url, REMOTE_DESCRIPTOR_PATH);
        let (status, body) = self.request_json(Method::GET, &url, None, None).await?;
        if !status.is_success() {
            return Err(rejected(status, &body));
        }
        serde_json::from_slice(&body)
            .map_err(|error| RemoteHostClientError::InvalidResponse(error.to_string()))
    }

    async fn pair(
        &self,
        base_url: &str,
        request: &PairWireRequest,
    ) -> Result<PairWireResponse, RemoteHostClientError> {
        let url = join_url(base_url, REMOTE_PAIR_PATH);
        let body = serde_json::to_vec(request)
            .map_err(|error| RemoteHostClientError::InvalidResponse(error.to_string()))?;
        let (status, body) = self
            .request_json(Method::POST, &url, None, Some(body))
            .await?;
        if !status.is_success() {
            return Err(rejected(status, &body));
        }
        serde_json::from_slice(&body)
            .map_err(|error| RemoteHostClientError::InvalidResponse(error.to_string()))
    }

    async fn validate_token(
        &self,
        base_url: &str,
        token: &str,
    ) -> Result<bool, RemoteHostClientError> {
        let url = join_url(base_url, REMOTE_SESSION_PATH);
        let (status, body) = self
            .request_json(Method::GET, &url, Some(token), None)
            .await?;
        if status.is_success() {
            return Ok(true);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Ok(false);
        }
        Err(rejected(status, &body))
    }

    async fn revoke_token(&self, base_url: &str, token: &str) -> Result<(), RemoteHostClientError> {
        let url = join_url(base_url, REMOTE_REVOKE_PATH);
        let (status, body) = self
            .request_json(Method::POST, &url, Some(token), None)
            .await?;
        if revoke_completed(status) {
            return Ok(());
        }
        Err(rejected(status, &body))
    }

    async fn invoke(
        &self,
        base_url: &str,
        token: &str,
        request: &InvokeWireRequest,
    ) -> Result<RemoteHttpResponse, RemoteHostClientError> {
        let url = join_url(base_url, REMOTE_INVOKE_PATH);
        let payload = serde_json::to_vec(request)
            .map_err(|error| RemoteHostClientError::InvalidResponse(error.to_string()))?;
        let (status, headers, body) = self
            .send(
                Method::POST,
                &url,
                Some(token),
                &[("Content-Type".to_string(), "application/json".to_string())],
                Some(payload),
                self.dispatch_timeout,
            )
            .await?;
        // Non-success is NOT an error here: the taxonomy mapping needs the status.
        Ok(RemoteHttpResponse {
            status: status.as_u16(),
            headers,
            body: body_text(body),
        })
    }

    async fn fetch(
        &self,
        base_url: &str,
        token: &str,
        request: &RemoteFetchRequest,
    ) -> Result<RemoteHttpResponse, RemoteHostClientError> {
        let url = join_url(base_url, &request.path);
        let method = Method::from_bytes(request.method.as_bytes()).map_err(|error| {
            RemoteHostClientError::InvalidResponse(format!("invalid HTTP method: {error}"))
        })?;
        let (status, headers, body) = self
            .send(
                method,
                &url,
                Some(token),
                &request.headers,
                request.body.clone().map(String::into_bytes),
                self.dispatch_timeout,
            )
            .await?;
        Ok(RemoteHttpResponse {
            status: status.as_u16(),
            headers,
            body: body_text(body),
        })
    }
}

/// Fallback client used when TLS roots are unavailable at AppState construction
/// (mirrors `UnavailableAtlassianApiClient`): every call fails typed-unreachable.
pub struct UnavailableRemoteHostClient {
    reason: String,
}

impl UnavailableRemoteHostClient {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

#[async_trait]
impl RemoteHostClient for UnavailableRemoteHostClient {
    async fn fetch_descriptor(
        &self,
        _base_url: &str,
    ) -> Result<EnvironmentDescriptor, RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }

    async fn pair(
        &self,
        _base_url: &str,
        _request: &PairWireRequest,
    ) -> Result<PairWireResponse, RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }

    async fn validate_token(
        &self,
        _base_url: &str,
        _token: &str,
    ) -> Result<bool, RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }

    async fn revoke_token(
        &self,
        _base_url: &str,
        _token: &str,
    ) -> Result<(), RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }

    async fn invoke(
        &self,
        _base_url: &str,
        _token: &str,
        _request: &InvokeWireRequest,
    ) -> Result<RemoteHttpResponse, RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }

    async fn fetch(
        &self,
        _base_url: &str,
        _token: &str,
        _request: &RemoteFetchRequest,
    ) -> Result<RemoteHttpResponse, RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }
}

// ============================================================================
// Test mock — the "mock host" the pairing/reconciler tests run against
// ============================================================================

/// Scripted response for one mock-host call.
pub type MockHostResult<T> = Result<T, RemoteHostClientError>;

/// Recorded call log entry for assertion in tests.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedHostCall {
    Descriptor {
        base_url: String,
    },
    Pair {
        base_url: String,
        request: PairWireRequest,
    },
    Validate {
        base_url: String,
        token: String,
    },
    Revoke {
        base_url: String,
        token: String,
    },
    Invoke {
        base_url: String,
        token: String,
        request: InvokeWireRequest,
    },
    Fetch {
        base_url: String,
        token: String,
        request: RemoteFetchRequest,
    },
}

/// Mock host for pairing/reconciler tests: scripted responses + a recorded call log.
pub struct MockRemoteHostClient {
    pub descriptor: Mutex<MockHostResult<EnvironmentDescriptor>>,
    pub pair_response: Mutex<MockHostResult<PairWireResponse>>,
    pub validate_response: Mutex<MockHostResult<bool>>,
    pub revoke_response: Mutex<MockHostResult<()>>,
    pub invoke_response: Mutex<MockHostResult<RemoteHttpResponse>>,
    pub fetch_response: Mutex<MockHostResult<RemoteHttpResponse>>,
    pub calls: Mutex<Vec<RecordedHostCall>>,
}

/// The default scripted dispatch answer: a `Read` command that returned `Ok(null)`.
fn default_invoke_response() -> RemoteHttpResponse {
    RemoteHttpResponse {
        status: 200,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: r#"{"ok":true,"result":null}"#.to_string(),
    }
}

impl MockRemoteHostClient {
    pub fn new(descriptor: EnvironmentDescriptor, pair_response: PairWireResponse) -> Self {
        Self {
            descriptor: Mutex::new(Ok(descriptor)),
            pair_response: Mutex::new(Ok(pair_response)),
            validate_response: Mutex::new(Ok(true)),
            revoke_response: Mutex::new(Ok(())),
            invoke_response: Mutex::new(Ok(default_invoke_response())),
            fetch_response: Mutex::new(Ok(default_invoke_response())),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// A mock host whose every surface fails with `Unreachable`.
    pub fn unreachable() -> Self {
        let error = RemoteHostClientError::Unreachable("mock host offline".to_string());
        Self {
            descriptor: Mutex::new(Err(error.clone())),
            pair_response: Mutex::new(Err(error.clone())),
            validate_response: Mutex::new(Err(error.clone())),
            revoke_response: Mutex::new(Err(error.clone())),
            invoke_response: Mutex::new(Err(error.clone())),
            fetch_response: Mutex::new(Err(error)),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Scripts the next dispatch answer (status + raw body), for taxonomy tests.
    pub fn script_invoke(&self, status: u16, body: impl Into<String>) {
        *self.invoke_response.lock().expect("mock invoke poisoned") = Ok(RemoteHttpResponse {
            status,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: body.into(),
        });
    }

    /// Scripts the next dispatch failure (timeout / connection refused).
    pub fn script_invoke_error(&self, error: RemoteHostClientError) {
        *self.invoke_response.lock().expect("mock invoke poisoned") = Err(error);
    }

    /// Scripts the next remounted-route answer.
    pub fn script_fetch(&self, status: u16, body: impl Into<String>) {
        self.script_fetch_with_headers(status, body, vec![("content-type", "application/json")]);
    }

    /// Scripts the next remounted-route answer together with the host's raw response
    /// headers, so response-allowlist behaviour is testable end to end.
    pub fn script_fetch_with_headers(
        &self,
        status: u16,
        body: impl Into<String>,
        headers: Vec<(&str, &str)>,
    ) {
        *self.fetch_response.lock().expect("mock fetch poisoned") = Ok(RemoteHttpResponse {
            status,
            headers: headers
                .into_iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.to_string()))
                .collect(),
            body: body.into(),
        });
    }

    pub fn recorded_calls(&self) -> Vec<RecordedHostCall> {
        self.calls.lock().expect("mock call log poisoned").clone()
    }

    fn record(&self, call: RecordedHostCall) {
        self.calls
            .lock()
            .expect("mock call log poisoned")
            .push(call);
    }
}

#[async_trait]
impl RemoteHostClient for MockRemoteHostClient {
    async fn fetch_descriptor(
        &self,
        base_url: &str,
    ) -> Result<EnvironmentDescriptor, RemoteHostClientError> {
        self.record(RecordedHostCall::Descriptor {
            base_url: base_url.to_string(),
        });
        self.descriptor
            .lock()
            .expect("mock descriptor poisoned")
            .clone()
    }

    async fn pair(
        &self,
        base_url: &str,
        request: &PairWireRequest,
    ) -> Result<PairWireResponse, RemoteHostClientError> {
        self.record(RecordedHostCall::Pair {
            base_url: base_url.to_string(),
            request: request.clone(),
        });
        self.pair_response
            .lock()
            .expect("mock pair response poisoned")
            .clone()
    }

    async fn validate_token(
        &self,
        base_url: &str,
        token: &str,
    ) -> Result<bool, RemoteHostClientError> {
        self.record(RecordedHostCall::Validate {
            base_url: base_url.to_string(),
            token: token.to_string(),
        });
        self.validate_response
            .lock()
            .expect("mock validate response poisoned")
            .clone()
    }

    async fn revoke_token(&self, base_url: &str, token: &str) -> Result<(), RemoteHostClientError> {
        self.record(RecordedHostCall::Revoke {
            base_url: base_url.to_string(),
            token: token.to_string(),
        });
        self.revoke_response
            .lock()
            .expect("mock revoke response poisoned")
            .clone()
    }

    async fn invoke(
        &self,
        base_url: &str,
        token: &str,
        request: &InvokeWireRequest,
    ) -> Result<RemoteHttpResponse, RemoteHostClientError> {
        self.record(RecordedHostCall::Invoke {
            base_url: base_url.to_string(),
            token: token.to_string(),
            request: request.clone(),
        });
        self.invoke_response
            .lock()
            .expect("mock invoke response poisoned")
            .clone()
    }

    async fn fetch(
        &self,
        base_url: &str,
        token: &str,
        request: &RemoteFetchRequest,
    ) -> Result<RemoteHttpResponse, RemoteHostClientError> {
        self.record(RecordedHostCall::Fetch {
            base_url: base_url.to_string(),
            token: token.to_string(),
            request: request.clone(),
        });
        self.fetch_response
            .lock()
            .expect("mock fetch response poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::to_bytes;
    use axum::extract::{Request as AxumRequest, State};
    use axum::routing::any;
    use axum::Router;

    #[derive(Clone)]
    struct CannedResponse {
        status: StatusCode,
        body: &'static str,
        seen: Arc<Mutex<Vec<SeenRequest>>>,
    }

    #[derive(Debug)]
    struct SeenRequest {
        method: Method,
        path: String,
        authorization: Option<String>,
        custom_header: Option<String>,
        body: String,
    }

    async fn canned_handler(
        State(state): State<CannedResponse>,
        request: AxumRequest,
    ) -> (StatusCode, String) {
        let method = request.method().clone();
        let path = request.uri().path().to_string();
        let authorization = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let custom_header = request
            .headers()
            .get("x-test-header")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = to_bytes(request.into_body(), 64 * 1024)
            .await
            .expect("request body")
            .to_vec();
        state.seen.lock().expect("seen requests").push(SeenRequest {
            method,
            path,
            authorization,
            custom_header,
            body: String::from_utf8(body).expect("utf8 request body"),
        });
        (state.status, state.body.to_string())
    }

    async fn spawn_canned_server(
        status: StatusCode,
        body: &'static str,
    ) -> (
        String,
        Arc<Mutex<Vec<SeenRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let state = CannedResponse {
            status,
            body,
            seen: Arc::clone(&seen),
        };
        let app = Router::new()
            .fallback(any(canned_handler))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind server");
        let address = listener.local_addr().expect("server address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve canned host");
        });
        (format!("http://{address}"), seen, task)
    }

    fn pair_wire_request() -> PairWireRequest {
        PairWireRequest {
            pairing_code: "rxp_code".to_string(),
            device_name: "Desktop".to_string(),
            client_version: "1.0.0".to_string(),
            requested_scopes: vec![Scope::UiRead],
        }
    }

    // C-11: parity tests use REAL serialization shapes — explicit camelCase keys,
    // protocol scope strings — not mock-convenient shapes.
    #[test]
    fn pair_request_serializes_the_documented_wire_shape() {
        let request = PairWireRequest {
            pairing_code: "rxp_0123456789abcdefghijklmnopqrstuv".to_string(),
            device_name: "RalphX Desktop".to_string(),
            client_version: "0.81.0".to_string(),
            requested_scopes: vec![Scope::UiRead, Scope::UiOperate],
        };
        let json = serde_json::to_value(&request).expect("request should serialize");
        assert_eq!(
            json,
            serde_json::json!({
                "pairingCode": "rxp_0123456789abcdefghijklmnopqrstuv",
                "deviceName": "RalphX Desktop",
                // §3.1 defines clientVersion and the host audits it; pin it so it cannot be
                // dropped again.
                "clientVersion": "0.81.0",
                "requestedScopes": ["ui:read", "ui:operate"],
            })
        );
    }

    #[test]
    fn pair_response_parses_the_documented_wire_shape() {
        let raw = r#"{
            "deviceToken": "rxd_live_secret",
            "deviceId": "device-1",
            "scopes": ["ui:read", "ui:operate"],
            "environmentId": "env-1",
            "protocolVersion": 1
        }"#;
        let response: PairWireResponse = serde_json::from_str(raw).expect("response should parse");
        assert_eq!(response.device_token, "rxd_live_secret");
        assert_eq!(response.device_id, "device-1");
        assert_eq!(response.scopes, vec![Scope::UiRead, Scope::UiOperate]);
        assert_eq!(response.environment_id, "env-1");
        assert_eq!(response.protocol_version, Some(1));
    }

    /// A host older than the `protocolVersion` field must still pair; the descriptor's copy
    /// is the fallback.
    #[test]
    fn pair_response_tolerates_a_host_without_protocol_version() {
        let raw = r#"{
            "deviceToken": "rxd_live_secret",
            "deviceId": "device-1",
            "scopes": ["ui:read"],
            "environmentId": "env-1"
        }"#;
        let response: PairWireResponse = serde_json::from_str(raw).expect("response should parse");
        assert_eq!(response.protocol_version, None);
    }

    /// The revoke seam must not read "route not mounted" as "token revoked": 404 comes from
    /// the remote router's fallback, while a mounted route answers 401/403 for a dead bearer
    /// (§6.1 / P-27 — a revoke reported as done but never performed orphans a live bearer).
    #[test]
    fn only_a_host_that_answered_counts_as_a_completed_revoke() {
        assert!(revoke_completed(StatusCode::OK));
        assert!(revoke_completed(StatusCode::NO_CONTENT));
        assert!(revoke_completed(StatusCode::UNAUTHORIZED));
        assert!(revoke_completed(StatusCode::FORBIDDEN));
        assert!(
            !revoke_completed(StatusCode::NOT_FOUND),
            "404 means the route is not mounted, not that the token died"
        );
        assert!(!revoke_completed(StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[test]
    fn join_url_tolerates_trailing_slashes() {
        assert_eq!(
            join_url("https://mac-studio.tailnet.ts.net/", REMOTE_PAIR_PATH),
            "https://mac-studio.tailnet.ts.net/remote/v1/auth/pair"
        );
        assert_eq!(
            join_url("http://100.101.102.103:3849", REMOTE_DESCRIPTOR_PATH),
            "http://100.101.102.103:3849/.well-known/ralphx/environment"
        );
    }

    #[tokio::test]
    async fn unavailable_client_returns_its_reason_from_every_surface() {
        let client = UnavailableRemoteHostClient::new("TLS roots missing");
        let invoke = InvokeWireRequest {
            request_id: "request-1".to_string(),
            cmd: "health_check".to_string(),
            args: serde_json::json!({}),
        };
        let fetch = RemoteFetchRequest {
            path: "/api/tasks".to_string(),
            method: "GET".to_string(),
            headers: vec![],
            body: None,
        };

        let errors = [
            client.fetch_descriptor("http://host").await.unwrap_err(),
            client
                .pair("http://host", &pair_wire_request())
                .await
                .unwrap_err(),
            client
                .validate_token("http://host", "token")
                .await
                .unwrap_err(),
            client
                .revoke_token("http://host", "token")
                .await
                .unwrap_err(),
            client
                .invoke("http://host", "token", &invoke)
                .await
                .unwrap_err(),
            client
                .fetch("http://host", "token", &fetch)
                .await
                .unwrap_err(),
        ];
        assert!(errors.into_iter().all(|error| matches!(
            error,
            RemoteHostClientError::Unreachable(reason) if reason == "TLS roots missing"
        )));
    }

    #[tokio::test]
    async fn hyper_client_parses_descriptor_and_pair_successes() {
        let descriptor_json = r#"{"environmentId":"env-1","appVersion":"1.0.0","protocolVersion":1,"minClientProtocol":1,"platform":"macos"}"#;
        let (base_url, seen, task) = spawn_canned_server(StatusCode::OK, descriptor_json).await;
        let client = HyperRemoteHostClient::new().expect("client");
        let descriptor = client
            .fetch_descriptor(&base_url)
            .await
            .expect("descriptor");
        assert_eq!(descriptor.environment_id, "env-1");
        assert_eq!(seen.lock().expect("seen")[0].path, REMOTE_DESCRIPTOR_PATH);
        task.abort();

        let pair_json = r#"{"deviceToken":"token","deviceId":"device-1","scopes":["ui:read"],"environmentId":"env-1","protocolVersion":1}"#;
        let (base_url, seen, task) = spawn_canned_server(StatusCode::OK, pair_json).await;
        let response = client
            .pair(&base_url, &pair_wire_request())
            .await
            .expect("pair");
        assert_eq!(response.device_token, "token");
        let seen = seen.lock().expect("seen");
        assert_eq!(seen[0].method, Method::POST);
        assert_eq!(seen[0].path, REMOTE_PAIR_PATH);
        assert!(seen[0].body.contains("pairingCode"));
        task.abort();
    }

    #[tokio::test]
    async fn hyper_client_classifies_rejections_and_invalid_json() {
        let client = HyperRemoteHostClient::new().expect("client");
        let (base_url, _, task) =
            spawn_canned_server(StatusCode::BAD_REQUEST, "bad pairing code").await;
        assert!(matches!(
            client.pair(&base_url, &pair_wire_request()).await,
            Err(RemoteHostClientError::Rejected { status: 400, message })
                if message == "bad pairing code"
        ));
        task.abort();

        let (base_url, _, task) = spawn_canned_server(StatusCode::OK, "not-json").await;
        assert!(matches!(
            client.fetch_descriptor(&base_url).await,
            Err(RemoteHostClientError::InvalidResponse(_))
        ));
        assert!(matches!(
            client.pair(&base_url, &pair_wire_request()).await,
            Err(RemoteHostClientError::InvalidResponse(_))
        ));
        task.abort();
    }

    #[tokio::test]
    async fn hyper_client_validates_and_revokes_with_bearer_status_semantics() {
        let client = HyperRemoteHostClient::new().expect("client");
        for (status, expected) in [
            (StatusCode::OK, true),
            (StatusCode::UNAUTHORIZED, false),
            (StatusCode::FORBIDDEN, false),
        ] {
            let (base_url, seen, task) = spawn_canned_server(status, "session").await;
            assert_eq!(
                client
                    .validate_token(&base_url, "secret")
                    .await
                    .expect("classified validation"),
                expected
            );
            assert_eq!(
                seen.lock().expect("seen")[0].authorization.as_deref(),
                Some("Bearer secret")
            );
            task.abort();
        }
        let (base_url, _, task) =
            spawn_canned_server(StatusCode::INTERNAL_SERVER_ERROR, "broken").await;
        assert!(matches!(
            client.validate_token(&base_url, "secret").await,
            Err(RemoteHostClientError::Rejected { status: 500, .. })
        ));
        task.abort();

        for status in [
            StatusCode::NO_CONTENT,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
        ] {
            let (base_url, _, task) = spawn_canned_server(status, "").await;
            client
                .revoke_token(&base_url, "secret")
                .await
                .expect("completed revoke");
            task.abort();
        }
        let (base_url, _, task) = spawn_canned_server(StatusCode::NOT_FOUND, "missing route").await;
        assert!(matches!(
            client.revoke_token(&base_url, "secret").await,
            Err(RemoteHostClientError::Rejected { status: 404, .. })
        ));
        task.abort();
    }

    #[tokio::test]
    async fn invoke_and_fetch_preserve_non_success_status_and_request_shape() {
        let client = HyperRemoteHostClient::new().expect("client");
        let (base_url, seen, task) = spawn_canned_server(StatusCode::FORBIDDEN, "denied").await;
        let invoke = InvokeWireRequest {
            request_id: "request-1".to_string(),
            cmd: "list_tasks".to_string(),
            args: serde_json::json!({"projectId": "p-1"}),
        };
        let response = client
            .invoke(&base_url, "secret", &invoke)
            .await
            .expect("invoke response");
        assert_eq!(response.status, 403);
        assert_eq!(response.body, "denied");
        // Real response headers, lowercased. Asserted by membership rather than by
        // equality: this runs against a live server whose `date` header is, by design,
        // different on every run.
        assert!(
            response
                .headers
                .iter()
                .any(|(name, value)| name == "content-type" && value.starts_with("text/plain")),
            "expected a forwarded content-type, got {:?}",
            response.headers
        );
        assert!(
            response
                .headers
                .iter()
                .all(|(name, _)| name.chars().all(|ch| !ch.is_ascii_uppercase())),
            "header names must be lowercased: {:?}",
            response.headers
        );
        assert_eq!(
            seen.lock().expect("seen")[0].authorization.as_deref(),
            Some("Bearer secret")
        );
        task.abort();

        let (base_url, seen, task) =
            spawn_canned_server(StatusCode::INTERNAL_SERVER_ERROR, "failed").await;
        let request = RemoteFetchRequest {
            path: "/api/tasks/task-1".to_string(),
            method: "PUT".to_string(),
            headers: vec![("x-test-header".to_string(), "custom".to_string())],
            body: Some("payload".to_string()),
        };
        let response = client
            .fetch(&base_url, "secret", &request)
            .await
            .expect("fetch response");
        assert_eq!(response.status, 500);
        assert_eq!(response.body, "failed");
        let seen = seen.lock().expect("seen");
        assert_eq!(seen[0].method, Method::PUT);
        assert_eq!(seen[0].path, "/api/tasks/task-1");
        assert_eq!(seen[0].authorization.as_deref(), Some("Bearer secret"));
        assert_eq!(seen[0].custom_header.as_deref(), Some("custom"));
        assert_eq!(seen[0].body, "payload");
        task.abort();
    }

    #[tokio::test]
    async fn hyper_client_maps_invalid_method_and_refused_connection() {
        let client = HyperRemoteHostClient::new().expect("client");
        let request = RemoteFetchRequest {
            path: "/api/tasks".to_string(),
            method: "bad method".to_string(),
            headers: vec![],
            body: None,
        };
        assert!(matches!(
            client.fetch("http://127.0.0.1:1", "secret", &request).await,
            Err(RemoteHostClientError::InvalidResponse(_))
        ));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        drop(listener);
        assert!(matches!(
            client.fetch_descriptor(&format!("http://{address}")).await,
            Err(RemoteHostClientError::Unreachable(_))
        ));
    }
}
